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
    pub fn set_registers(&mut self, values: &[u64], out: &mut Vec<u8>) -> Result<(), EncodeError> {
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
mod tests;
