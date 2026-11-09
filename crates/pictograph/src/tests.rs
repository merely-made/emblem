// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use super::*;
use emblem::{Call, Host, Paint, Palette, Recorder, Rgba, decode_metadata, execute};

fn addresses() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = (0..64u8).map(|i| vec![i]).collect();
    out.push(b"".to_vec());
    out.push(b"hello".to_vec());
    out.push(b"a rather longer content address, 32 bytes+".to_vec());
    out.push(vec![0xFF; 64]);
    out
}

fn run(file: &[u8], palette: &Palette, height: f32) -> Vec<Call> {
    let (_, start) = decode_metadata(file).expect("metadata");
    let mut sink = Recorder::default();
    execute(
        file,
        start,
        palette,
        Host {
            features: 0,
            height,
        },
        &mut sink,
    )
    .expect("execution");
    sink.calls
}

fn palette_of(colors: &[(usize, Rgba)]) -> Palette {
    let mut entries = Palette::default().0;
    for (index, color) in colors {
        entries[*index] = *color;
    }
    Palette::new(entries).unwrap()
}

fn fills(calls: &[Call]) -> Vec<Rgba> {
    calls
        .iter()
        .filter_map(|c| match c {
            Call::Fill(Paint::Flat(color)) => Some(*color),
            _ => None,
        })
        .collect()
}

/// Encode the coarse cells as a stable binary mask, row-major from bit 0.
fn small_mask(cells: [[bool; SMALL_GRID]; SMALL_GRID]) -> u16 {
    cells.iter().enumerate().fold(0, |mask, (row, cells)| {
        cells.iter().enumerate().fold(mask, |mask, (col, filled)| {
            mask | (u16::from(*filled) << (row * SMALL_GRID + col))
        })
    })
}

/// Recover the 3x3 mask from the decoded low-detail arm. This checks the
/// actual IconVG choice at Canvas's normal face height, rather than only
/// checking the generator's intermediate representation.
fn decoded_small_mask(calls: &[Call]) -> u16 {
    let mut mask = 0;
    for call in calls {
        let Call::MoveTo(x, y) = call else {
            continue;
        };
        let col = ((*x as i32 - ORIGIN) / SMALL_CELL) as usize;
        let row = ((*y as i32 - ORIGIN) / SMALL_CELL) as usize;
        assert!(col < SMALL_GRID && row < SMALL_GRID, "move at ({x}, {y})");
        mask |= 1 << (row * SMALL_GRID + col);
    }
    mask
}

// ---- determinism --------------------------------------------------------

#[test]
fn the_same_address_always_gives_the_same_bytes() {
    for address in addresses() {
        let once = derive(&address).unwrap();
        for _ in 0..8 {
            assert_eq!(derive(&address).unwrap(), once, "address {address:?}");
        }
    }
}

/// A digest of a face, for pinning it without a 200-byte literal.
///
/// Deliberately the same FNV-1a the derivation seeds from: one exactly
/// specified function, reused, rather than a second one to keep correct.
fn digest(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The digest-pinned fixtures: committed values pinning the derivation.
///
/// These literals are the point. A test that recomputed both sides would
/// pass no matter how the grammar changed, which is no test at all. When
/// this fails, the mapping has moved and **every peer's faces have
/// changed** — so the response is to bump [`DERIVATION_VERSION`]
/// deliberately and then update these, never to quietly update these.
#[test]
fn the_derivation_matches_its_digest_pinned_fixtures() {
    // (address, byte length, digest, filled cells)
    const GOLDEN: [(&[u8], usize, u64, usize); 4] = [
        (b"", 83, 0x9B94_E95C_C9B2_B632, 6),
        (b"a", 195, 0xEA2E_8F42_9779_FD95, 13),
        (b"pictograph", 186, 0x81B8_7D5C_1E2D_21B1, 13),
        (b"\x00\x01\x02\x03", 131, 0xB15F_8757_0B5F_5CCD, 9),
    ];
    for (address, len, want, filled) in GOLDEN {
        let face = derive(address).unwrap();
        assert_eq!(face.len(), len, "length for {address:?}");
        assert_eq!(digest(&face), want, "digest for {address:?}");
        assert_eq!(
            params_of(address).filled_count(),
            filled,
            "cells for {address:?}"
        );
    }
}

#[test]
fn an_address_face_is_the_same_by_every_route() {
    for address in addresses() {
        let face = derive(&address).unwrap();
        assert_eq!(derive_in(&KeyDomain::Address, &address).unwrap(), face);
        assert_eq!(FaceKey::address(address.clone()).derive().unwrap(), face);
        assert_eq!(
            FaceKey::address(address.clone()).params(),
            params_of(&address)
        );
    }
}

#[test]
fn the_same_key_under_two_domains_gives_two_faces() {
    for address in addresses() {
        let as_address = derive(&address).unwrap();
        let in_one = derive_in(&KeyDomain::Context("forum".into()), &address).unwrap();
        let in_two = derive_in(&KeyDomain::Context("market".into()), &address).unwrap();
        assert_ne!(in_one, as_address, "{address:?}");
        assert_ne!(in_one, in_two, "{address:?}");
    }
    // The context's length is hashed, so moving bytes across the boundary
    // between context and key is a different face.
    let split_one = derive_in(&KeyDomain::Context("ab".into()), b"c").unwrap();
    let split_two = derive_in(&KeyDomain::Context("a".into()), b"bc").unwrap();
    assert_ne!(split_one, split_two);
}

#[test]
fn a_face_key_names_grammar_versions_and_domain() {
    let address = FaceKey::address(b"mere://note".to_vec());
    assert_eq!(address.grammar.id, "cells5");
    assert_eq!(address.grammar.seed_version, DERIVATION_VERSION);
    let redrawn = FaceKey {
        grammar: Grammar {
            drawing_version: CELLS5.drawing_version + 1,
            ..CELLS5
        },
        ..address.clone()
    };
    let elsewhere = FaceKey {
        domain: KeyDomain::Context("forum".into()),
        ..address.clone()
    };
    assert_ne!(redrawn, address, "a drawing bump misses the cache");
    assert_ne!(elsewhere, address, "a domain is part of the key");
    // A drawing version leaves identity alone: the parameters do not move.
    assert_eq!(redrawn.params(), address.params());
}

#[test]
fn the_grammar_does_not_collapse_to_one_variant() {
    // A mapping bug that always picked the same symmetry or form would
    // still pass every other test here while making every face alike.
    let mut symmetries = std::collections::HashSet::new();
    let mut forms = std::collections::HashSet::new();
    let mut primaries = std::collections::HashSet::new();
    for address in addresses() {
        let p = params_of(&address);
        symmetries.insert(format!("{:?}", p.symmetry));
        forms.insert(format!("{:?}", p.form));
        primaries.insert(p.primary);
    }
    assert_eq!(symmetries.len(), 3, "all three symmetries must occur");
    assert_eq!(forms.len(), 2, "both forms must occur");
    assert!(
        primaries.len() >= 6,
        "only {} distinct primary colours across the address set",
        primaries.len(),
    );
}

#[test]
fn different_addresses_generally_give_different_faces() {
    let mut seen = std::collections::HashSet::new();
    for address in addresses() {
        seen.insert(derive(&address).unwrap());
    }
    // Collisions are possible in principle; a near-total spread is the
    // property worth asserting.
    assert!(
        seen.len() >= addresses().len() * 3 / 4,
        "only {} distinct faces from {} addresses",
        seen.len(),
        addresses().len(),
    );
}

// ---- theming ------------------------------------------------------------

#[test]
fn one_face_wears_two_palettes() {
    // The property the whole design rests on: identical bytes, different
    // palettes, different colours — with no re-derivation.
    let face = derive(b"theme me").unwrap();
    let params = params_of(b"theme me");

    let red = Rgba::new(0xFF, 0x00, 0x00, 0xFF);
    let blue = Rgba::new(0x00, 0x00, 0xFF, 0xFF);
    let light = palette_of(&[(usize::from(params.primary), red)]);
    let dark = palette_of(&[(usize::from(params.primary), blue)]);

    assert!(fills(&run(&face, &light, 8.0)).contains(&red));
    assert!(fills(&run(&face, &dark, 8.0)).contains(&blue));
}

#[test]
fn every_fill_resolves_to_a_palette_entry_never_a_literal() {
    // A literal colour would render identically under every palette, which
    // is exactly the bug this crate must not have. Give the palette
    // distinctive colours and check nothing else appears.
    let markers: Vec<Rgba> = (0..PALETTE_SPAN)
        .map(|i| Rgba::new(i * 16 + 1, 0x00, 0x00, 0xFF))
        .collect();
    let palette = palette_of(
        &markers
            .iter()
            .enumerate()
            .map(|(i, c)| (i, *c))
            .collect::<Vec<_>>(),
    );

    for address in addresses() {
        let face = derive(&address).unwrap();
        for height in [8.0, 64.0] {
            for color in fills(&run(&face, &palette, height)) {
                assert!(
                    markers.contains(&color),
                    "address {address:?} at height {height} filled with {color:?}, \
                         which is not a palette entry",
                );
            }
        }
    }
}

// ---- level of detail ----------------------------------------------------

#[test]
fn each_face_draws_a_simpler_arm_when_small() {
    let mut simpler = 0;
    for address in addresses() {
        let face = derive(&address).unwrap();
        let small = run(&face, &Palette::default(), 8.0);
        let large = run(&face, &Palette::default(), 128.0);

        // The small arm combines its coarse cells into one flat fill.
        assert_eq!(fills(&small).len(), 1, "address {address:?}");
        assert!(!fills(&large).is_empty(), "address {address:?}");

        let ops = |calls: &[Call]| calls.len();
        if ops(&large) > ops(&small) {
            simpler += 1;
        }
    }
    // A single-cell face can tie; most must genuinely simplify.
    assert!(
        simpler >= addresses().len() * 3 / 4,
        "only {simpler} of {} faces simplified when small",
        addresses().len(),
    );
}

#[test]
fn the_arms_switch_at_the_threshold() {
    let face = derive(b"threshold").unwrap();
    let below = run(&face, &Palette::default(), LOD_THRESHOLD - 0.1);
    let at = run(&face, &Palette::default(), LOD_THRESHOLD);
    assert_ne!(below, at, "the arm must change at the threshold");
    assert_eq!(fills(&below).len(), 1, "below is the single bold shape");
}

#[test]
fn the_default_height_small_arm_keeps_distinct_coarse_masks() {
    // Canvas renders an ordinary 36px node face at 25.92px after its
    // inset. A test at the LOD extreme would miss an accidental change to
    // the actual product size.
    const DEFAULT_FACE_HEIGHT: f32 = 25.92;

    let mut masks = std::collections::HashMap::<u16, usize>::new();
    for address in addresses() {
        let expected = small_mask(small_cells(&params_of(&address)));
        let face = derive(&address).unwrap();
        let decoded = decoded_small_mask(&run(&face, &Palette::default(), DEFAULT_FACE_HEIGHT));
        assert_eq!(decoded, expected, "decoded mask for {address:?}");
        *masks.entry(decoded).or_default() += 1;
    }

    let unique = masks.len();
    let largest_collision = masks.values().copied().max().unwrap_or(0);

    // This corpus currently produces 30 masks, with at most eight faces
    // sharing one. Keep a little room for an intentional future grammar
    // change while preventing a return to the v2 one-rectangle arm.
    assert!(
        unique >= 30,
        "only {unique} distinct default-height small masks"
    );
    assert!(
        largest_collision <= 8,
        "largest default-height mask collision was {largest_collision}"
    );
}

// ---- structure ----------------------------------------------------------

#[test]
fn a_face_is_never_blank() {
    for address in addresses() {
        assert!(
            params_of(&address).filled_count() > 0,
            "address {address:?}"
        );
    }
}

#[test]
fn symmetry_holds_for_every_address() {
    for address in addresses() {
        let p = params_of(&address);
        for row in 0..GRID {
            for col in 0..GRID {
                let (mr, mc) = (GRID - 1 - row, GRID - 1 - col);
                let here = p.cells[row][col];
                match p.symmetry {
                    Symmetry::MirrorX => {
                        assert_eq!(here, p.cells[row][mc], "mirror-X at ({row},{col})");
                    },
                    Symmetry::MirrorBoth => {
                        assert_eq!(
                            here, p.cells[row][mc],
                            "mirror-both horizontal at ({row},{col})"
                        );
                        assert_eq!(
                            here, p.cells[mr][col],
                            "mirror-both vertical at ({row},{col})"
                        );
                    },
                    Symmetry::Rotate180 => {
                        assert_eq!(here, p.cells[mr][mc], "rotate-180 at ({row},{col})");
                    },
                }
            }
        }
    }
}

#[test]
fn independent_regions_sample_each_orbit_once() {
    for (symmetry, expected) in [
        (Symmetry::MirrorX, 15),
        (Symmetry::MirrorBoth, 9),
        (Symmetry::Rotate180, 13),
    ] {
        let sampled = (0..GRID)
            .flat_map(|row| (0..GRID).map(move |col| (row, col)))
            .filter(|&(row, col)| is_independent_cell(symmetry, row, col))
            .count();
        assert_eq!(sampled, expected, "{symmetry:?}");

        for row in 0..GRID {
            for col in 0..GRID {
                let (mr, mc) = (GRID - 1 - row, GRID - 1 - col);
                let orbit: std::collections::HashSet<_> = match symmetry {
                    Symmetry::MirrorX => [(row, col), (row, mc)].into_iter().collect(),
                    Symmetry::MirrorBoth => [(row, col), (row, mc), (mr, col), (mr, mc)]
                        .into_iter()
                        .collect(),
                    Symmetry::Rotate180 => [(row, col), (mr, mc)].into_iter().collect(),
                };
                let representatives = orbit
                    .into_iter()
                    .filter(|&(orbit_row, orbit_col)| {
                        is_independent_cell(symmetry, orbit_row, orbit_col)
                    })
                    .count();
                assert_eq!(
                    representatives, 1,
                    "{symmetry:?} orbit containing ({row},{col})"
                );
            }
        }
    }
}

#[test]
fn the_two_colours_are_always_distinct() {
    for address in addresses() {
        let p = params_of(&address);
        assert_ne!(p.primary, p.accent, "address {address:?}");
        assert!(p.primary < PALETTE_SPAN && p.accent < PALETTE_SPAN);
    }
}

#[test]
fn faces_stay_small() {
    for address in addresses() {
        let face = derive(&address).unwrap();
        assert!(
            face.len() < 512,
            "address {address:?} produced {} bytes",
            face.len(),
        );
    }
}

// ---- fuzz ---------------------------------------------------------------

#[test]
fn random_addresses_produce_decodable_faces() {
    let mut stream = Stream(0x1234_5678_9ABC_DEF0);
    for case in 0..300 {
        let len = (stream.below(48) + 1) as usize;
        let address: Vec<u8> = (0..len).map(|_| (stream.below(256)) as u8).collect();
        let face = derive(&address).unwrap();
        // Decoding must not panic or error at either extreme.
        for height in [0.0, 1.0, LOD_THRESHOLD, 4096.0] {
            let calls = run(&face, &Palette::default(), height);
            assert!(!calls.is_empty(), "case {case} at height {height}");
        }
    }
}
