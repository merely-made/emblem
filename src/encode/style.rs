// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Writing register and fill ops.
//!
//! # `SEL` moves under you, so the writer tracks it
//!
//! Register and fill ops do not name a register outright. They name a *slot*,
//! the low four bits of the opcode, and the register is `REGS[SEL + slot]` —
//! and several of the ops then change `SEL` as a side effect:
//!
//! - a register-set op with slot `0` decrements `SEL` **after** writing;
//! - the multi-register op decrements `SEL` by the count **before** writing,
//!   then fills upward from `SEL + 1`;
//! - a fill op with slot `0` increments `SEL` **before** reading.
//!
//! An encoder that did not model this would emit ops that address the wrong
//! registers as soon as any slot-`0` op appeared, and the mistake would be
//! invisible until something rendered in the wrong colour. [`StyleWriter`]
//! therefore keeps `SEL` in lockstep with [`crate::Registers`], which starts it
//! at 56 so that `REGS[SEL + 8]` reaches the custom palette's first entry.
//!
//! [`StyleWriter::register_index`] answers "where would slot `n` land right
//! now", which is what a caller needs to lay a palette out.
//!
//! # Theming lives here
//!
//! Registers are pre-loaded from the palette the *caller* supplies at decode
//! time, so a fill that names a register is already themeable: the same bytes
//! resolve differently under a different palette. Writing a literal colour into
//! a register with [`StyleWriter::set_color`] opts out of that, which is
//! occasionally right and usually not.

use super::color::ColorSource;
use super::{EncodeError, color, num};
use crate::color::Rgba;
use crate::sink::{GradientKind, Spread};

/// `SEL`'s value when a graphic starts, matching [`crate::Registers::new`].
pub const INITIAL_SEL: u8 = 56;

/// The largest number of registers one multi-register op can write.
pub const MAX_REGISTERS_PER_OP: usize = 17;

/// The largest number of stops a gradient fill can name.
pub const MAX_GRADIENT_STOPS: usize = 64;

/// Writes register and fill ops, tracking `SEL` exactly as the VM does.
#[derive(Clone, Copy, Debug)]
pub struct StyleWriter {
    sel: u8,
}

impl Default for StyleWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleWriter {
    /// A writer positioned where a graphic begins.
    pub fn new() -> Self {
        Self { sel: INITIAL_SEL }
    }

    /// The current `SEL`.
    pub fn sel(&self) -> u8 {
        self.sel
    }

    /// Which register slot `slot` currently addresses, modulo 64.
    ///
    /// This is the value *before* any side effect the next op may have.
    pub fn register_index(&self, slot: u8) -> u8 {
        self.sel.wrapping_add(slot) & 0x3F
    }

    /// Move `SEL` by `delta`, modulo 64. Emits op `0x36`.
    pub fn adjust_sel(&mut self, delta: u8, out: &mut Vec<u8>) {
        out.push(0x36);
        out.push(delta);
        self.sel = self.sel.wrapping_add(delta) & 0x3F;
    }

    /// Write a whole 64-bit register into slot `slot`.
    ///
    /// Slot `0` also decrements `SEL`, per the specification.
    pub fn set_register(
        &mut self,
        slot: u8,
        value: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let slot = check_slot(slot)?;
        out.push(0x60 | slot);
        out.extend_from_slice(&(value as u32).to_le_bytes());
        out.extend_from_slice(&((value >> 32) as u32).to_le_bytes());
        self.decrement_sel_if_slot_zero(slot);
        Ok(())
    }

    /// Write only a register's low half, zeroing the high half.
    pub fn set_register_low(
        &mut self,
        slot: u8,
        low: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let slot = check_slot(slot)?;
        out.push(0x40 | slot);
        out.extend_from_slice(&low.to_le_bytes());
        self.decrement_sel_if_slot_zero(slot);
        Ok(())
    }

    /// Write only a register's high half, zeroing the low half.
    pub fn set_register_high(
        &mut self,
        slot: u8,
        high: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let slot = check_slot(slot)?;
        out.push(0x50 | slot);
        out.extend_from_slice(&high.to_le_bytes());
        self.decrement_sel_if_slot_zero(slot);
        Ok(())
    }

    /// Write 2 to [`MAX_REGISTERS_PER_OP`] consecutive registers.
    ///
    /// This op decrements `SEL` by the count *first*, then writes upward from
    /// `SEL + 1`, so afterwards the values sit at slots `1..=count`.
    pub fn set_registers(
        &mut self,
        values: &[u64],
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        if values.len() < 2 || values.len() > MAX_REGISTERS_PER_OP {
            return Err(EncodeError::RegisterCountOutOfRange(values.len()));
        }
        out.push(0x70 | ((values.len() - 2) as u8));
        for value in values {
            out.extend_from_slice(&(*value as u32).to_le_bytes());
            out.extend_from_slice(&((*value >> 32) as u32).to_le_bytes());
        }
        self.sel = self.sel.wrapping_sub(values.len() as u8) & 0x3F;
        Ok(())
    }

    /// Put a literal colour, and a gradient stop position, into slot `slot`.
    ///
    /// The colour is baked into the file and cannot be re-themed; prefer
    /// leaving the palette-loaded register alone, or use [`Self::set_blend`]
    /// with palette references.
    pub fn set_color(
        &mut self,
        slot: u8,
        color: Rgba,
        stop: f32,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let value = color::register_value(color, stop)?;
        self.set_register(slot, value, out)
    }

    /// Put a blend of two colour references into slot `slot`.
    ///
    /// Palette references resolve at decode time, so this stays themeable.
    pub fn set_blend(
        &mut self,
        slot: u8,
        zero: ColorSource,
        one: ColorSource,
        blend: u8,
        stop: f32,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let value = color::blend_register_value(zero, one, blend, stop)?;
        self.set_register(slot, value, out)
    }

    /// Fill the accumulated path with the colour in slot `slot`.
    ///
    /// Slot `0` increments `SEL` *before* the register is read.
    pub fn fill_flat(&mut self, slot: u8, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let slot = check_slot(slot)?;
        self.increment_sel_if_slot_zero(slot);
        out.push(0x80 | slot);
        Ok(())
    }

    /// Fill the accumulated path with a gradient whose stops are read from
    /// `stop_count` consecutive registers starting at slot `slot`.
    ///
    /// `nominal` is the gradient's transform: three values for a linear
    /// gradient, six for a radial one. Slot `0` increments `SEL` first, as with
    /// a flat fill.
    pub fn fill_gradient(
        &mut self,
        slot: u8,
        kind: GradientKind,
        spread: Spread,
        stop_count: usize,
        nominal: &[f32],
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let slot = check_slot(slot)?;
        if !(2..=MAX_GRADIENT_STOPS).contains(&stop_count) {
            return Err(EncodeError::GradientStopsOutOfRange(stop_count));
        }
        let wanted = match kind {
            GradientKind::Linear => 3,
            GradientKind::Radial => 6,
        };
        if nominal.len() != wanted {
            return Err(EncodeError::GradientMatrixWrongLength {
                expected: wanted,
                found: nominal.len(),
            });
        }

        self.increment_sel_if_slot_zero(slot);
        let base = match kind {
            GradientKind::Linear => 0x90,
            GradientKind::Radial => 0xA0,
        };
        out.push(base | slot);
        // The config byte: spread in the top two bits, `stops - 2` below.
        out.push((spread_bits(spread) << 6) | ((stop_count - 2) as u8));
        for value in nominal {
            num::float32(*value, out)?;
        }
        Ok(())
    }

    fn decrement_sel_if_slot_zero(&mut self, slot: u8) {
        if slot == 0 {
            self.sel = self.sel.wrapping_sub(1) & 0x3F;
        }
    }

    fn increment_sel_if_slot_zero(&mut self, slot: u8) {
        if slot == 0 {
            self.sel = self.sel.wrapping_add(1) & 0x3F;
        }
    }
}

/// The two bits a spread occupies in a gradient's config byte.
fn spread_bits(spread: Spread) -> u8 {
    match spread {
        Spread::None => 0x00,
        Spread::Pad => 0x01,
        Spread::Reflect => 0x02,
        Spread::Repeat => 0x03,
    }
}

fn check_slot(slot: u8) -> Result<u8, EncodeError> {
    if slot > 0x0F {
        return Err(EncodeError::SlotOutOfRange(slot));
    }
    Ok(slot)
}

#[cfg(test)]
mod tests {
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
            w.set_register(slot, color::register_value(c(0, 0, 0, 0xFF), 0.0).unwrap(), &mut out)
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
        w.set_blend(2, ColorSource::Custom(5), ColorSource::Custom(5), 0, 0.0, &mut out)
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
        w.set_register_high(4, color::rgba_to_le_u32(green), &mut out).unwrap();
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
            w.fill_gradient(1, GradientKind::Linear, spread, 2, &[1.0, 0.0, 0.0], &mut out)
                .unwrap();

            match one_fill(&out, &palette) {
                Paint::Gradient { kind, spread: got, stops, .. } => {
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
                color::blend_register_value(
                    ColorSource::Custom(9),
                    ColorSource::Custom(9),
                    0,
                    0.0,
                )
                .unwrap(),
                color::register_value(c(0, 0, 0, 0xFF), 1.0 - 1.0 / 65536.0).unwrap(),
            ],
            &mut out,
        )
        .unwrap();
        path(&mut out);
        w.fill_gradient(1, GradientKind::Linear, Spread::Pad, 2, &[1.0, 0.0, 0.0], &mut out)
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
        assert_eq!(w.set_register(16, 0, &mut out), Err(EncodeError::SlotOutOfRange(16)));
        assert_eq!(w.fill_flat(16, &mut out), Err(EncodeError::SlotOutOfRange(16)));
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
            w.fill_gradient(1, GradientKind::Linear, Spread::Pad, 65, &[0.0; 3], &mut out),
            Err(EncodeError::GradientStopsOutOfRange(65))
        );
        assert_eq!(
            w.fill_gradient(1, GradientKind::Radial, Spread::Pad, 2, &[0.0; 3], &mut out),
            Err(EncodeError::GradientMatrixWrongLength { expected: 6, found: 3 })
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
        w.fill_gradient(1, GradientKind::Linear, Spread::Pad, stops, &[0.0; 3], &mut out)
            .unwrap();
        let config = out[out.len() - 13];
        assert_eq!(config & 0x3F, 62, "the largest legal stop encoding");
        assert_ne!(config & 0x3F, 63, "63 is reserved and must never be emitted");
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
}
