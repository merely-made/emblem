// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Palette slots as CSS variables.
//!
//! The rule under test: a colour is written as a variable exactly when it came
//! from a custom-palette slot, and its fallback is the colour the snapshot
//! writes. A literal that happens to equal a slot's colour, and a blend of
//! slots, both stay literal.

#![cfg(feature = "svg")]

use emblem::encode::Writer;
use emblem::encode::color::ColorSource;
use emblem::{
    GradientKind, Host, IdPrefix, Palette, Rgba, Spread, SvgOptions, Theme, ViewBox, to_svg,
};

const RED: Rgba = Rgba::new(0xff, 0, 0, 0xff);
/// Premultiplied; straight `#0080ff` at alpha `0x80`.
const HALF_BLUE: Rgba = Rgba::new(0, 0x40, 0x80, 0x80);

fn palette(first: Rgba, second: Rgba) -> Palette {
    let mut colors = [Rgba::new(0, 0, 0, 0xff); 64];
    colors[0] = first;
    colors[1] = second;
    Palette::new(colors).unwrap()
}

/// Five fills: slot 0, a literal equal to slot 0's colour, a blend of slots 0
/// and 1, a gradient over slots 0 and 1, and slot 1.
fn graphic() -> Vec<u8> {
    let mut w = Writer::new(ViewBox {
        min_x: 0.0,
        min_y: 0.0,
        max_x: 10.0,
        max_y: 10.0,
    });
    let square = |w: &mut Writer| {
        w.move_to(1.0, 1.0)?;
        w.line_to(&[(9.0, 1.0), (9.0, 9.0), (1.0, 9.0)])
    };
    square(&mut w).unwrap();
    w.fill_flat(8).unwrap();
    square(&mut w).unwrap();
    w.set_color(1, RED, 0.0).unwrap();
    w.fill_flat(1).unwrap();
    square(&mut w).unwrap();
    w.set_blend(2, ColorSource::Custom(0), ColorSource::Custom(1), 0x80, 0.0)
        .unwrap();
    w.fill_flat(2).unwrap();
    square(&mut w).unwrap();
    let linear = [0.125, 0.0, -0.125];
    w.fill_gradient(8, GradientKind::Linear, Spread::Pad, 2, &linear)
        .unwrap();
    square(&mut w).unwrap();
    w.fill_flat(9).unwrap();
    w.finish().unwrap()
}

fn write(palette: &Palette, theme: Theme) -> String {
    let mut options = SvgOptions::default();
    options.theme = theme;
    to_svg(&graphic(), palette, Host::default(), &options).unwrap()
}

fn vars() -> Theme {
    Theme::CssVars {
        prefix: IdPrefix::default(),
    }
}

/// The `<path>` lines, in order.
fn paths(svg: &str) -> Vec<&str> {
    svg.lines().filter(|l| l.starts_with("<path")).collect()
}

/// Every `#rrggbb` replaced by `#`, leaving the structure.
fn shape(svg: &str) -> String {
    let mut out = String::new();
    let mut chars = svg.chars();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '#' && !out.ends_with("url(#") {
            chars.by_ref().take(6).for_each(drop);
        }
    }
    out
}

#[test]
fn slot_colours_become_variables_with_the_snapshot_as_fallback() {
    let p = palette(RED, HALF_BLUE);
    let snapshot = write(&p, Theme::Snapshot);
    let themed = write(&p, vars());
    let (snap, them) = (paths(&snapshot), paths(&themed));
    assert_eq!(snap.len(), 5);

    assert!(snap[0].contains(r##"fill="#ff0000""##), "{}", snap[0]);
    assert!(
        them[0].contains(r##"style="fill:var(--emblem-slot-0,#ff0000)""##),
        "{}",
        them[0]
    );
    // Opacity is baked from the palette given, never themed.
    assert!(snap[4].contains(r##"fill="#0080ff" fill-opacity="0.502""##));
    assert!(
        them[4].contains(r##"style="fill:var(--emblem-slot-1,#0080ff)" fill-opacity="0.502""##),
        "{}",
        them[4]
    );
}

#[test]
fn literals_and_blends_stay_literal() {
    let p = palette(RED, HALF_BLUE);
    let snapshot = write(&p, Theme::Snapshot);
    let themed = write(&p, vars());
    let (snap, them) = (paths(&snapshot), paths(&themed));
    // A literal red that equals slot 0's red, and a blend of slots 0 and 1.
    assert_eq!(snap[1], them[1]);
    assert_eq!(snap[2], them[2]);
    assert!(!them[1].contains("var(") && !them[2].contains("var("));
}

#[test]
fn gradient_stops_from_slots_become_variables() {
    let themed = write(&palette(RED, HALF_BLUE), vars());
    let stops: Vec<&str> = themed.lines().filter(|l| l.starts_with("<stop")).collect();
    assert!(stops[0].contains(r##"style="stop-color:var(--emblem-slot-0,#ff0000)""##));
    let last = stops.last().unwrap();
    assert!(
        last.contains(r##"style="stop-color:var(--emblem-slot-1,#0080ff)" stop-opacity="0.502""##),
        "{last}"
    );
    // The eight stops between differ in alpha at both ends: blends, literal.
    assert_eq!(stops.len(), 10);
    assert!(stops[1..9].iter().all(|s| !s.contains("var(")));
}

#[test]
fn a_new_palette_moves_colours_but_not_the_themed_structure() {
    let (a, b) = (
        palette(RED, HALF_BLUE),
        palette(Rgba::new(0, 0xff, 0, 0xff), HALF_BLUE),
    );
    assert_ne!(write(&a, Theme::Snapshot), write(&b, Theme::Snapshot));
    assert_eq!(shape(&write(&a, vars())), shape(&write(&b, vars())));
}

#[test]
fn the_variable_prefix_is_used() {
    let theme = Theme::CssVars {
        prefix: IdPrefix::new("face-").unwrap(),
    };
    let themed = write(&palette(RED, HALF_BLUE), theme);
    assert!(themed.contains("var(--face-slot-0,"), "{themed}");
}
