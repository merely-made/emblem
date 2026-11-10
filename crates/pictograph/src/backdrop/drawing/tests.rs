// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use emblem::{Call, Host, Paint, Palette, Recorder, Rgba, decode_metadata, execute};

use super::*;
use crate::backdrop::{BackdropNode, LATTICE, MAX_CLUSTERS, OTHER};

fn node(position: [f32; 2]) -> BackdropNode<'static> {
    BackdropNode {
        position,
        kind: BackdropKind::Image,
        community: Some("held"),
        site: Some("example"),
        visit_seconds: None,
    }
}

fn calls(bytes: &[u8], palette: &Palette, height: f32) -> Vec<Call> {
    let (_, start) = decode_metadata(bytes).unwrap();
    let mut recorder = Recorder::default();
    execute(
        bytes,
        start,
        palette,
        Host {
            height,
            ..Host::default()
        },
        &mut recorder,
    )
    .unwrap();
    recorder.calls
}

fn digest() -> ViewDigest {
    ViewDigest::from_held([
        node([-63.0, 0.0]),
        node([-63.0, 65.0]),
        node([65.0, 65.0]),
        node([65.0, 0.0]),
    ])
    .unwrap()
}

#[test]
fn movement_inside_lattice_cells_and_insertion_order_keep_digest_and_bytes() {
    let first = digest();
    let mut reordered = [
        node([126.0, 63.0]),
        node([126.0, 126.0]),
        node([-1.0, 126.0]),
        node([-1.0, 63.0]),
    ];
    let second = ViewDigest::from_held(reordered).unwrap();
    assert_eq!(first.bounds(), [-2, -1, 3, 3]);
    assert_eq!(first, second);
    assert_eq!(
        derive_backdrop(&first).unwrap(),
        derive_backdrop(&second).unwrap()
    );
    reordered[0].position[0] += LATTICE as f32;
    assert_ne!(first, ViewDigest::from_held(reordered).unwrap());
    assert!(ViewDigest::from_held([node([f32::NAN, 0.0])]).is_none());
}

#[test]
fn overflow_groups_merge_deterministically_and_keep_the_full_envelope() {
    let names: Vec<_> = (0..20).map(|index| format!("c{index:02}")).collect();
    let nodes: Vec<_> = names
        .iter()
        .enumerate()
        .flat_map(|(index, name)| {
            [[0.0, 0.0], [64.0, 0.0], [64.0, 64.0], [0.0, 64.0]].map(|[x, y]| BackdropNode {
                community: Some(name),
                ..node([index as f32 * 128.0 + x, y])
            })
        })
        .collect();
    let digest = ViewDigest::from_held(nodes.iter().copied()).unwrap();
    let reverse = ViewDigest::from_held(nodes.iter().rev().copied()).unwrap();
    assert_eq!(digest, reverse);
    assert_eq!(digest.clusters.len(), MAX_CLUSTERS);
    let other = digest.clusters.last().unwrap();
    assert_eq!(other.entry, OTHER);
    assert_eq!(other.center, [34, 0]);
    assert!(other.hull.contains(&[39, 1]));
    let bytes = derive_backdrop(&digest).unwrap();
    assert_eq!(bytes, derive_backdrop(&reverse).unwrap());
    assert!(bytes.len() <= MAX_BYTES);
    let large = calls(&bytes, &Palette::default(), LOD_HEIGHT);
    assert_eq!(
        large
            .iter()
            .filter(|call| matches!(call, Call::MoveTo(..)))
            .count(),
        16 * 3 + 9
    );
}

#[test]
fn kinds_and_activity_are_quantized_from_materialized_facts() {
    use BackdropKind::*;
    assert_eq!(
        BackdropKind::classify(Some("image/svg+xml"), |_| false),
        Image
    );
    assert_eq!(
        BackdropKind::classify(Some("TEXT/CSV; charset=utf-8"), |_| false),
        Data
    );
    assert_eq!(
        BackdropKind::classify(Some("application/pdf"), |_| false),
        Document
    );
    assert_eq!(
        BackdropKind::classify(Some("audio/ogg"), |tag| tag == "collection"),
        Collection
    );
    assert_eq!(BackdropKind::classify(None, |tag| tag == "video"), Video);
    assert_eq!(BackdropKind::classify(None, |_| false), Other);
    let first = BackdropNode {
        visit_seconds: Some(100.0),
        ..node([0.0, 0.0])
    };
    for (span, bucket) in [
        (0.0, 0),
        (1.0, 1),
        (3_600.0, 1),
        (3_601.0, 2),
        (86_401.0, 3),
    ] {
        let last = BackdropNode {
            visit_seconds: Some(100.0 + span),
            ..node([65.0, 0.0])
        };
        assert_eq!(
            ViewDigest::from_held([first, last]).unwrap().activity,
            bucket
        );
    }
}

#[test]
fn both_versions_change_bytes_and_both_arms_decode_with_host_colors() {
    let digest = digest();
    let bytes = derive_backdrop(&digest).unwrap();
    for bumped in [
        crate::Grammar {
            seed_version: WASH.seed_version + 1,
            ..WASH
        },
        crate::Grammar {
            drawing_version: WASH.drawing_version + 1,
            ..WASH
        },
    ] {
        let versioned = encode(&digest, bumped, Budget::new(MAX_PATHS, MAX_BYTES)).unwrap();
        assert_ne!(bytes, versioned);
        assert_eq!(
            calls(&bytes, &Palette::default(), 0.0),
            calls(&versioned, &Palette::default(), 0.0)
        );
    }
    let color = Rgba::new(40, 8, 2, 42);
    let mut palette = Palette([color; 64]);
    palette.0[CLEAR as usize] = Rgba::new(0, 0, 0, 0);
    let small = calls(&bytes, &palette, LOD_HEIGHT - 1.0);
    let large = calls(&bytes, &palette, LOD_HEIGHT);
    assert_eq!(&large[..small.len()], small);
    assert!(matches!(large.last(), Some(Call::Fill(Paint::Flat(fill))) if *fill == color));
    let Some((stops, matrix)) = small.iter().find_map(|call| match call {
        Call::Fill(Paint::Gradient {
            kind: GradientKind::Radial,
            stops,
            matrix,
            ..
        }) => Some((stops, matrix)),
        _ => None,
    }) else {
        panic!("expected a radial wash")
    };
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].color, color);
    assert_eq!(stops[2].color.a, 0);
    assert!(matrix.inverse().is_some());
    assert_ne!(small, calls(&bytes, &Palette::default(), LOD_HEIGHT - 1.0));
}

#[test]
fn output_ceilings_fail_closed_at_the_boundary() {
    let mut budget = Budget::new(MAX_PATHS, MAX_BYTES);
    assert!(budget.take_paths(MAX_PATHS).is_ok());
    assert!(matches!(
        budget.take_paths(1),
        Err(BackdropError::TooManyPaths)
    ));
    assert!(budget.finish(vec![0; MAX_BYTES]).is_ok());
    assert!(matches!(
        Budget::new(MAX_PATHS, MAX_BYTES).finish(vec![0; MAX_BYTES + 1]),
        Err(BackdropError::TooManyBytes)
    ));
    let digest = digest();
    let bytes = derive_backdrop(&digest).unwrap();
    assert!(matches!(
        encode(&digest, WASH, Budget::new(11, MAX_BYTES)),
        Err(BackdropError::TooManyPaths)
    ));
    assert!(matches!(
        encode(&digest, WASH, Budget::new(MAX_PATHS, bytes.len() - 1)),
        Err(BackdropError::TooManyBytes)
    ));
}
