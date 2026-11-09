// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! SVG text, compared verbatim against committed expectations.
//!
//! The text is meant to be content-addressed, so any byte that moves here is a
//! change every consumer sees; the expected strings are the contract.

#![cfg(feature = "svg")]

use emblem::encode::Writer;
use emblem::{
    Error, GradientKind, Host, IdPrefix, Matrix, Paint, Palette, Rgba, Sink, Spread, Stop,
    SvgError, SvgOptions, SvgSink, ViewBox, to_svg,
};

/// The specification's worked example: Material Design's "action/info" icon.
const ACTION_INFO: [u8; 36] = [
    0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33, 0x59,
    0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d,
    0x75, 0x7d, 0x6d, 0x88,
];

const RED: Rgba = Rgba::new(0xff, 0, 0, 0xff);
const BLUE: Rgba = Rgba::new(0, 0, 0xff, 0xff);
/// `t = x/8 - 1/8`: 0 at `x = 1`, 1 at `x = 9`.
const LINEAR: [f32; 6] = [0.125, 0.0, -0.125, 0.0, 0.0, 0.0];
/// Centre `(5, 5)`, radius 4.
const RADIAL: [f32; 6] = [0.25, 0.0, -1.25, 0.0, 0.25, -1.25];
const SQUARE: &str =
    r#"<path d="M1 1 L9 1 L9 9 L1 9 Z" fill="url(#emblem-g0)" fill-rule="nonzero""#;

fn svg(file: &[u8]) -> String {
    to_svg(
        file,
        &Palette::default(),
        Host::default(),
        &SvgOptions::default(),
    )
    .expect("svg")
}

fn ten() -> ViewBox {
    ViewBox {
        min_x: 0.0,
        min_y: 0.0,
        max_x: 10.0,
        max_y: 10.0,
    }
}

fn gradient(kind: GradientKind, spread: Spread, matrix: [f32; 6], to: Rgba) -> Paint {
    Paint::Gradient {
        kind,
        spread,
        stops: vec![
            Stop {
                offset: 0.0,
                color: RED,
            },
            Stop {
                offset: 1.0,
                color: to,
            },
        ],
        matrix: Matrix(matrix),
    }
}

fn square(paint: &Paint) -> Result<String, SvgError> {
    let mut sink = SvgSink::new(ten(), &SvgOptions::default());
    sink.move_to(1.0, 1.0);
    sink.line_to(9.0, 1.0);
    sink.line_to(9.0, 9.0);
    sink.line_to(1.0, 9.0);
    sink.close();
    sink.fill(paint);
    sink.finish()
}

/// A document holding one `defs` body and the square.
fn document(defs: &str, path_tail: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\n<defs>\n{defs}</defs>\n{SQUARE}{path_tail}/>\n</svg>\n"
    )
}

const RED_TO_BLUE: &str = "<stop offset=\"0\" stop-color=\"#ff0000\"/>\n\
<stop offset=\"1\" stop-color=\"#0000ff\"/>\n";

#[test]
fn the_specifications_own_example_writes_as_expected() {
    let want = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-24 -24 48 48\">\n\
<path d=\"M0 -20 C-11.0357 -20 -20 -11.0357 -20 0 C-20 11.0357 -11.0357 20 0 20 \
C11.0357 20 20 11.0357 20 0 C20 -11.0357 11.0357 -20 0 -20 Z \
M2 10 L-2 10 L-2 -2 L2 -2 L2 10 Z M2 -6 L-2 -6 L-2 -10 L2 -10 L2 -6 Z\" \
fill=\"#000000\" fill-rule=\"nonzero\"/>\n</svg>\n";
    assert_eq!(svg(&ACTION_INFO), want);
}

#[test]
fn every_segment_kind_and_a_translucent_fill_write_as_expected() {
    let mut sink = SvgSink::new(ten(), &SvgOptions::default());
    sink.move_to(1.0, 2.0);
    sink.quad_to(3.0, 4.0, 5.0, 6.0);
    sink.cube_to(7.0, 8.0, 9.0, 1.5, 2.25, 0.125);
    sink.close();
    // Premultiplied (0x40, 0, 0x20) at alpha 0x80 is straight #800040.
    sink.fill(&Paint::Flat(Rgba::new(0x40, 0, 0x20, 0x80)));
    let want = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\n\
<path d=\"M1 2 Q3 4 5 6 C7 8 9 1.5 2.25 0.125 Z\" fill=\"#800040\" fill-opacity=\"0.502\" \
fill-rule=\"nonzero\"/>\n</svg>\n";
    assert_eq!(sink.finish().unwrap(), want);
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    assert_eq!(svg(&ACTION_INFO), svg(&ACTION_INFO));
    let paint = gradient(GradientKind::Radial, Spread::None, RADIAL, BLUE);
    assert_eq!(square(&paint), square(&paint));
}

#[test]
fn the_host_height_chooses_the_level_of_detail_arm() {
    let mut w = Writer::new(ten());
    w.level_of_detail(0.0, 32.0, |arm| {
        arm.move_to(0.0, 0.0)?;
        arm.line_to(&[(8.0, 8.0)])?;
        arm.fill_flat(8)
    })
    .unwrap();
    w.level_of_detail(32.0, f32::INFINITY, |arm| {
        arm.move_to(0.0, 0.0)?;
        arm.line_to(&[(2.0, 4.0), (8.0, 8.0)])?;
        arm.fill_flat(8)
    })
    .unwrap();
    let file = w.finish().unwrap();
    let at = |height| {
        let host = Host {
            features: 0,
            height,
        };
        to_svg(&file, &Palette::default(), host, &SvgOptions::default()).unwrap()
    };
    assert!(at(16.0).contains(r#"d="M0 0 L8 8 Z""#), "{}", at(16.0));
    assert!(at(48.0).contains(r#"d="M0 0 L2 4 L8 8 Z""#), "{}", at(48.0));
}

#[test]
fn an_unwritable_coordinate_or_file_is_an_error() {
    let mut sink = SvgSink::new(ten(), &SvgOptions::default());
    sink.move_to(f32::NAN, 0.0);
    sink.fill(&Paint::Flat(RED));
    assert_eq!(sink.finish(), Err(SvgError::NonFinite));

    let opts = SvgOptions::default();
    let bad = to_svg(b"nope", &Palette::default(), Host::default(), &opts);
    assert_eq!(bad, Err(SvgError::Decode(Error::BadMagic)));
}

#[test]
fn linear_gradients_write_as_expected_for_every_spread() {
    let gradient_open = "<linearGradient id=\"emblem-g0\" gradientUnits=\"userSpaceOnUse\" \
x1=\"1\" y1=\"0\" x2=\"9\" y2=\"0\" spreadMethod=";
    for (spread, method) in [
        (Spread::Pad, "pad"),
        (Spread::Reflect, "reflect"),
        (Spread::Repeat, "repeat"),
    ] {
        let defs = format!("{gradient_open}\"{method}\">\n{RED_TO_BLUE}</linearGradient>\n");
        let got = square(&gradient(GradientKind::Linear, spread, LINEAR, BLUE)).unwrap();
        assert_eq!(got, document(&defs, ""), "{spread:?}");
    }

    // The strip is t in [0, 1] along x and covers the square's y in [1, 9].
    let defs = format!(
        "<clipPath id=\"emblem-c0\" clipPathUnits=\"userSpaceOnUse\"><rect \
transform=\"matrix(8 0 0 8 1 0)\" x=\"0\" y=\"0.125\" width=\"1\" height=\"1\"/></clipPath>\n\
{gradient_open}\"pad\">\n{RED_TO_BLUE}</linearGradient>\n"
    );
    let got = square(&gradient(GradientKind::Linear, Spread::None, LINEAR, BLUE)).unwrap();
    assert_eq!(got, document(&defs, r#" clip-path="url(#emblem-c0)""#));
}

#[test]
fn radial_gradients_write_as_expected_for_every_spread() {
    let gradient_open = "<radialGradient id=\"emblem-g0\" gradientUnits=\"userSpaceOnUse\" \
cx=\"0\" cy=\"0\" r=\"1\" gradientTransform=\"matrix(4 0 0 4 5 5)\" spreadMethod=";
    for (spread, method) in [
        (Spread::Pad, "pad"),
        (Spread::Reflect, "reflect"),
        (Spread::Repeat, "repeat"),
    ] {
        let defs = format!("{gradient_open}\"{method}\">\n{RED_TO_BLUE}</radialGradient>\n");
        let got = square(&gradient(GradientKind::Radial, spread, RADIAL, BLUE)).unwrap();
        assert_eq!(got, document(&defs, ""), "{spread:?}");
    }

    let defs = format!(
        "<clipPath id=\"emblem-c0\" clipPathUnits=\"userSpaceOnUse\"><circle \
transform=\"matrix(4 0 0 4 5 5)\" cx=\"0\" cy=\"0\" r=\"1\"/></clipPath>\n\
{gradient_open}\"pad\">\n{RED_TO_BLUE}</radialGradient>\n"
    );
    let got = square(&gradient(GradientKind::Radial, Spread::None, RADIAL, BLUE)).unwrap();
    assert_eq!(got, document(&defs, r#" clip-path="url(#emblem-c0)""#));
}

#[test]
fn only_spread_none_writes_a_clip() {
    for kind in [GradientKind::Linear, GradientKind::Radial] {
        let matrix = if kind == GradientKind::Linear {
            LINEAR
        } else {
            RADIAL
        };
        for spread in [Spread::None, Spread::Pad, Spread::Reflect, Spread::Repeat] {
            let got = square(&gradient(kind, spread, matrix, BLUE)).unwrap();
            assert_eq!(
                got.contains("<clipPath"),
                spread == Spread::None,
                "{kind:?} {spread:?}"
            );
        }
    }
}

#[test]
fn a_span_changing_alpha_interpolates_premultiplied() {
    // Opaque red to transparent black. Premultiplied, the hue survives and
    // only alpha falls; straight SVG interpolation would darken to black.
    let got = square(&gradient(
        GradientKind::Linear,
        Spread::Pad,
        LINEAR,
        Rgba::TRANSPARENT,
    ));
    let stops = "<stop offset=\"0\" stop-color=\"#ff0000\"/>\n\
<stop offset=\"0.1111\" stop-color=\"#ff0000\" stop-opacity=\"0.8902\"/>\n\
<stop offset=\"0.2222\" stop-color=\"#ff0000\" stop-opacity=\"0.7765\"/>\n\
<stop offset=\"0.3333\" stop-color=\"#ff0000\" stop-opacity=\"0.6667\"/>\n\
<stop offset=\"0.4444\" stop-color=\"#ff0000\" stop-opacity=\"0.5569\"/>\n\
<stop offset=\"0.5556\" stop-color=\"#ff0000\" stop-opacity=\"0.4431\"/>\n\
<stop offset=\"0.6667\" stop-color=\"#ff0000\" stop-opacity=\"0.3333\"/>\n\
<stop offset=\"0.7778\" stop-color=\"#ff0000\" stop-opacity=\"0.2235\"/>\n\
<stop offset=\"0.8889\" stop-color=\"#ff0000\" stop-opacity=\"0.1098\"/>\n\
<stop offset=\"1\" stop-color=\"#000000\" stop-opacity=\"0\"/>\n";
    assert!(got.unwrap().contains(stops));
}

#[test]
fn degenerate_gradients_are_errors() {
    let flat_line = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    let paint = gradient(GradientKind::Linear, Spread::Pad, flat_line, BLUE);
    assert_eq!(square(&paint), Err(SvgError::DegenerateGradient));

    let singular = [1.0, 2.0, 0.0, 2.0, 4.0, 0.0];
    let paint = gradient(GradientKind::Radial, Spread::Pad, singular, BLUE);
    assert_eq!(square(&paint), Err(SvgError::DegenerateGradient));

    let Paint::Gradient { mut stops, .. } =
        gradient(GradientKind::Linear, Spread::Pad, LINEAR, BLUE)
    else {
        unreachable!()
    };
    stops[1].offset = f32::INFINITY;
    let paint = Paint::Gradient {
        kind: GradientKind::Linear,
        spread: Spread::Pad,
        stops,
        matrix: Matrix(LINEAR),
    };
    assert_eq!(square(&paint), Err(SvgError::NonFinite));
}

#[test]
fn id_prefixes_are_validated_and_used() {
    for bad in ["", "1a", "a b", "a\"", "é", &"a".repeat(33)] {
        assert_eq!(IdPrefix::new(bad), Err(SvgError::BadPrefix), "{bad:?}");
    }
    let mut options = SvgOptions::default();
    options.id_prefix = IdPrefix::new("_face-3").unwrap();
    let mut sink = SvgSink::new(ten(), &options);
    sink.move_to(1.0, 1.0);
    sink.fill(&gradient(GradientKind::Linear, Spread::None, LINEAR, BLUE));
    let got = sink.finish().unwrap();
    assert!(
        got.contains(r#"id="_face-3g0""#) && got.contains(r#"url(#_face-3c0)"#),
        "{got}"
    );
}
