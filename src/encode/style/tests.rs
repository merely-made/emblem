// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::color::Palette;
use crate::sink::{Call, Paint};
use crate::{Host, Recorder, execute};

fn c(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba::new(r, g, b, a)
}

/// A minimal path, so that a following fill has something to fill.
///
/// A fill op with no accumulated geometry is a no-op in the VM — it only
/// reaches the sink when a path is pending — so every fill test needs
/// this first. Op `0x01` is one line-to repetition and two coordinates.
fn path(out: &mut Vec<u8>) {
    out.push(0x01);
    num::coordinate(0.0, out).unwrap();
    num::coordinate(8.0, out).unwrap();
}

/// Run a bytecode fragment and collect what the sink was told.
fn run(bytes: &[u8], palette: &Palette) -> Vec<Call> {
    let mut sink = Recorder::default();
    execute(bytes, 0, palette, Host::default(), &mut sink).unwrap();
    sink.calls
}

fn one_fill(bytes: &[u8], palette: &Palette) -> Paint {
    let calls = run(bytes, palette);
    let fills: Vec<&Paint> = calls
        .iter()
        .filter_map(|call| match call {
            Call::Fill(paint) => Some(paint),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1, "expected exactly one fill, got {calls:?}");
    fills[0].clone()
}

fn palette_with(index: usize, color: Rgba) -> Palette {
    let mut colors = Palette::default().0;
    colors[index] = color;
    Palette::new(colors).unwrap()
}

// ---- SEL bookkeeping ----------------------------------------------------

#[test]
fn sel_starts_where_the_registers_do() {
    assert_eq!(StyleWriter::new().sel(), INITIAL_SEL);
    // So slot 8 addresses the custom palette's first entry.
    assert_eq!(StyleWriter::new().register_index(8), 0);
}

#[test]
fn a_slot_zero_register_write_decrements_sel_afterwards() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_register(0, 0, &mut out).unwrap();
    assert_eq!(w.sel(), INITIAL_SEL - 1);
}

#[test]
fn a_nonzero_slot_register_write_leaves_sel_alone() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_register(3, 0, &mut out).unwrap();
    assert_eq!(w.sel(), INITIAL_SEL);
}

#[test]
fn a_slot_zero_fill_increments_sel_before_reading() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.fill_flat(0, &mut out).unwrap();
    assert_eq!(w.sel(), INITIAL_SEL + 1);
}

#[test]
fn the_multi_register_op_decrements_sel_by_the_count() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(&[0, 0, 0], &mut out).unwrap();
    assert_eq!(w.sel(), INITIAL_SEL - 3);
}

/// The writer's `SEL` must agree with the VM's after the same ops. This is
/// the property the whole module exists to hold.
#[test]
fn the_writers_sel_tracks_the_vms_through_every_side_effecting_op() {
    let palette = Palette::default();
    for slot in [0u8, 1, 5, 15] {
        let mut w = StyleWriter::new();
        let mut out = Vec::new();
        w.set_register(
            slot,
            color::register_value(c(0, 0, 0, 0xFF), 0.0).unwrap(),
            &mut out,
        )
        .unwrap();
        w.adjust_sel(3, &mut out);
        w.set_registers(&[0, 0], &mut out).unwrap();
        path(&mut out);
        w.fill_flat(slot, &mut out).unwrap();

        // The VM must accept the sequence and produce one fill.
        let calls = run(&out, &palette);
        assert!(
            calls.iter().any(|call| matches!(call, Call::Fill(_))),
            "slot {slot} produced no fill: {calls:?}",
        );
    }
}

// ---- flat fills and theming ---------------------------------------------

#[test]
fn a_literal_colour_fills_with_that_colour() {
    let palette = Palette::default();
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    let green = c(0x00, 0xC0, 0x00, 0xC0);
    w.set_color(3, green, 0.0, &mut out).unwrap();
    path(&mut out);
    w.fill_flat(3, &mut out).unwrap();
    assert_eq!(one_fill(&out, &palette), Paint::Flat(green));
}

#[test]
fn a_palette_loaded_register_fills_from_the_callers_palette() {
    // Registers start pre-loaded from the palette, so a fill that names one
    // is themeable with no register write at all. Slot 8 is palette entry 0.
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    path(&mut out);
    w.fill_flat(8, &mut out).unwrap();

    let red = c(0xFF, 0x00, 0x00, 0xFF);
    let blue = c(0x00, 0x00, 0xFF, 0xFF);
    assert_eq!(one_fill(&out, &palette_with(0, red)), Paint::Flat(red));
    assert_eq!(one_fill(&out, &palette_with(0, blue)), Paint::Flat(blue));
}

#[test]
fn one_byte_sequence_wears_two_palettes_through_a_blend_reference() {
    // The same bytes, two palettes, two colours: the re-theming property
    // that justifies the whole format choice.
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_blend(
        2,
        ColorSource::Custom(5),
        ColorSource::Custom(5),
        0,
        0.0,
        &mut out,
    )
    .unwrap();
    path(&mut out);
    w.fill_flat(2, &mut out).unwrap();

    let light = c(0xFF, 0xFF, 0xFF, 0xFF);
    let dark = c(0x10, 0x10, 0x10, 0xFF);
    assert_eq!(one_fill(&out, &palette_with(5, light)), Paint::Flat(light));
    assert_eq!(one_fill(&out, &palette_with(5, dark)), Paint::Flat(dark));
}

#[test]
fn every_custom_palette_slot_is_reachable_by_a_fill() {
    for entry in 0..64usize {
        let mut w = StyleWriter::new();
        let mut out = Vec::new();
        w.set_blend(
            1,
            ColorSource::Custom(entry as u8),
            ColorSource::Custom(entry as u8),
            0,
            0.0,
            &mut out,
        )
        .unwrap();
        path(&mut out);
        w.fill_flat(1, &mut out).unwrap();

        let marker = c(0x11, 0x22, 0x33, 0xFF);
        assert_eq!(
            one_fill(&out, &palette_with(entry, marker)),
            Paint::Flat(marker),
            "custom entry {entry}",
        );
    }
}

#[test]
fn a_low_half_write_leaves_the_high_half_zero() {
    // Low-half-only writes zero the colour, giving transparent black.
    let palette = Palette::default();
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_register_low(4, 0x0000_8000, &mut out).unwrap();
    path(&mut out);
    w.fill_flat(4, &mut out).unwrap();
    assert_eq!(one_fill(&out, &palette), Paint::Flat(Rgba::TRANSPARENT));
}

#[test]
fn a_high_half_write_sets_the_colour() {
    let palette = Palette::default();
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    let green = c(0x00, 0xC0, 0x00, 0xC0);
    w.set_register_high(4, color::rgba_to_le_u32(green), &mut out)
        .unwrap();
    path(&mut out);
    w.fill_flat(4, &mut out).unwrap();
    assert_eq!(one_fill(&out, &palette), Paint::Flat(green));
}

#[test]
fn the_multi_register_op_lands_its_values_where_the_vm_reads_them() {
    // After the op, the values sit at slots 1..=count.
    let palette = Palette::default();
    let red = c(0xFF, 0x00, 0x00, 0xFF);
    let green = c(0x00, 0xFF, 0x00, 0xFF);
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(
        &[
            color::register_value(red, 0.0).unwrap(),
            color::register_value(green, 0.0).unwrap(),
        ],
        &mut out,
    )
    .unwrap();
    path(&mut out);
    w.fill_flat(1, &mut out).unwrap();
    assert_eq!(one_fill(&out, &palette), Paint::Flat(red));

    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(
        &[
            color::register_value(red, 0.0).unwrap(),
            color::register_value(green, 0.0).unwrap(),
        ],
        &mut out,
    )
    .unwrap();
    path(&mut out);
    path(&mut out);
    w.fill_flat(2, &mut out).unwrap();
    assert_eq!(one_fill(&out, &palette), Paint::Flat(green));
}

// ---- gradients ----------------------------------------------------------

#[test]
fn a_linear_gradient_round_trips_kind_spread_and_stops() {
    let palette = Palette::default();
    for spread in [Spread::None, Spread::Pad, Spread::Reflect, Spread::Repeat] {
        let mut w = StyleWriter::new();
        let mut out = Vec::new();
        let red = c(0xFF, 0x00, 0x00, 0xFF);
        let blue = c(0x00, 0x00, 0xFF, 0xFF);
        w.set_registers(
            &[
                color::register_value(red, 0.0).unwrap(),
                color::register_value(blue, 0.5).unwrap(),
            ],
            &mut out,
        )
        .unwrap();
        path(&mut out);
        w.fill_gradient(
            1,
            GradientKind::Linear,
            spread,
            2,
            &[1.0, 0.0, 0.0],
            &mut out,
        )
        .unwrap();

        match one_fill(&out, &palette) {
            Paint::Gradient {
                kind,
                spread: got,
                stops,
                ..
            } => {
                assert_eq!(kind, GradientKind::Linear);
                assert_eq!(got, spread, "spread round trip");
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].color, red);
                assert_eq!(stops[1].color, blue);
                assert_eq!(stops[0].offset, 0.0);
                assert_eq!(stops[1].offset, 0.5);
            },
            other => panic!("expected a gradient, got {other:?}"),
        }
    }
}

#[test]
fn a_radial_gradient_takes_six_nominal_values() {
    let palette = Palette::default();
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(
        &[
            color::register_value(c(0xFF, 0, 0, 0xFF), 0.0).unwrap(),
            color::register_value(c(0, 0, 0xFF, 0xFF), 0.75).unwrap(),
        ],
        &mut out,
    )
    .unwrap();
    path(&mut out);
    w.fill_gradient(
        1,
        GradientKind::Radial,
        Spread::Pad,
        2,
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        &mut out,
    )
    .unwrap();

    match one_fill(&out, &palette) {
        Paint::Gradient { kind, stops, .. } => {
            assert_eq!(kind, GradientKind::Radial);
            assert_eq!(stops.len(), 2);
            assert_eq!(stops[1].offset, 0.75);
        },
        other => panic!("expected a gradient, got {other:?}"),
    }
}

#[test]
fn gradient_stops_are_themeable_too() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(
        &[
            color::blend_register_value(ColorSource::Custom(9), ColorSource::Custom(9), 0, 0.0)
                .unwrap(),
            color::register_value(c(0, 0, 0, 0xFF), 1.0 - 1.0 / 65536.0).unwrap(),
        ],
        &mut out,
    )
    .unwrap();
    path(&mut out);
    w.fill_gradient(
        1,
        GradientKind::Linear,
        Spread::Pad,
        2,
        &[1.0, 0.0, 0.0],
        &mut out,
    )
    .unwrap();

    let marker = c(0x20, 0x40, 0x60, 0xFF);
    match one_fill(&out, &palette_with(9, marker)) {
        Paint::Gradient { stops, .. } => assert_eq!(stops[0].color, marker),
        other => panic!("expected a gradient, got {other:?}"),
    }
}

// ---- refusals -----------------------------------------------------------

#[test]
fn out_of_range_slots_counts_and_matrices_are_refused() {
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    assert_eq!(
        w.set_register(16, 0, &mut out),
        Err(EncodeError::SlotOutOfRange(16))
    );
    assert_eq!(
        w.fill_flat(16, &mut out),
        Err(EncodeError::SlotOutOfRange(16))
    );
    assert_eq!(
        w.set_registers(&[0], &mut out),
        Err(EncodeError::RegisterCountOutOfRange(1))
    );
    assert_eq!(
        w.set_registers(&[0; 18], &mut out),
        Err(EncodeError::RegisterCountOutOfRange(18))
    );
    assert_eq!(
        w.fill_gradient(1, GradientKind::Linear, Spread::Pad, 1, &[0.0; 3], &mut out),
        Err(EncodeError::GradientStopsOutOfRange(1))
    );
    assert_eq!(
        w.fill_gradient(
            1,
            GradientKind::Linear,
            Spread::Pad,
            65,
            &[0.0; 3],
            &mut out
        ),
        Err(EncodeError::GradientStopsOutOfRange(65))
    );
    assert_eq!(
        w.fill_gradient(1, GradientKind::Radial, Spread::Pad, 2, &[0.0; 3], &mut out),
        Err(EncodeError::GradientMatrixWrongLength {
            expected: 6,
            found: 3
        })
    );
}

#[test]
fn the_reserved_stop_count_is_unreachable() {
    // A config byte with 63 in its low six bits is invalid, and the stop
    // range stops short of producing one.
    let mut w = StyleWriter::new();
    let mut out = Vec::new();
    w.set_registers(&[0; 17], &mut out).unwrap();
    let stops = MAX_GRADIENT_STOPS;
    w.fill_gradient(
        1,
        GradientKind::Linear,
        Spread::Pad,
        stops,
        &[0.0; 3],
        &mut out,
    )
    .unwrap();
    let config = out[out.len() - 13];
    assert_eq!(config & 0x3F, 62, "the largest legal stop encoding");
    assert_ne!(
        config & 0x3F,
        63,
        "63 is reserved and must never be emitted"
    );
}

// ---- determinism --------------------------------------------------------

#[test]
fn style_emission_is_deterministic() {
    let build = || {
        let mut w = StyleWriter::new();
        let mut out = Vec::new();
        for slot in 0..16u8 {
            w.set_blend(
                slot,
                ColorSource::Custom(slot),
                ColorSource::Custom(slot.wrapping_add(1) & 0x3F),
                slot.wrapping_mul(7),
                f32::from(slot) / 16.0,
                &mut out,
            )
            .unwrap();
            w.fill_flat(slot, &mut out).unwrap();
        }
        out
    };
    let once = build();
    for _ in 0..8 {
        assert_eq!(once, build());
    }
}
