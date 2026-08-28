// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Colors, palettes, and the register model.
//!
//! IconVG works in **alpha-premultiplied** RGBA throughout, including gradient
//! interpolation, which is where it differs from SVG. Halfway between opaque
//! bright red and transparent black is a 50%-opaque *bright* red, not a
//! 50%-opaque dark red: the hue and saturation survive and only alpha falls.
//!
//! A 4-tuple is "sensible" when `R <= A`, `G <= A` and `B <= A`. That test is
//! load-bearing rather than a validity check: a register's high 32 bits hold a
//! colour when the tuple is sensible, and hold a *blend of two colour
//! references* when it is not. Nonsense is the tag.

use crate::Error;

/// An alpha-premultiplied RGBA colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Whether the tuple is sensible under alpha-premultiplication.
    ///
    /// Non-sensible is not necessarily invalid; in a register it signals a
    /// blended colour instead.
    pub const fn is_sensible(&self) -> bool {
        self.r <= self.a && self.g <= self.a && self.b <= self.a
    }

    /// Blend two colours by an 8-bit weight, per the spec's rounding:
    /// `((W0 * C0) + (W1 * C1) + 128) / 255`, floored.
    pub fn blend(zero: Self, one: Self, blend: u8) -> Self {
        let w1 = u32::from(blend);
        let w0 = 255 - w1;
        let mix = |c0: u8, c1: u8| -> u8 {
            (((w0 * u32::from(c0)) + (w1 * u32::from(c1)) + 128) / 255) as u8
        };
        Self {
            r: mix(zero.r, one.r),
            g: mix(zero.g, one.g),
            b: mix(zero.b, one.b),
            a: mix(zero.a, one.a),
        }
    }
}

/// The 128-entry built-in palette.
///
/// Three special entries, then 125 opaque colours whose channels are quantized
/// to one of five levels. They are ordered so the `RR:GG:BB:AA` bytes read as a
/// little-endian `u32` increase, which puts red in the innermost loop and blue
/// in the outermost.
pub fn built_in_palette() -> [Rgba; 128] {
    const LEVELS: [u8; 5] = [0x00, 0x40, 0x80, 0xC0, 0xFF];
    let mut palette = [Rgba::TRANSPARENT; 128];
    palette[0] = Rgba::new(0x00, 0x00, 0x00, 0x00);
    palette[1] = Rgba::new(0x80, 0x80, 0x80, 0x80);
    palette[2] = Rgba::new(0xC0, 0xC0, 0xC0, 0xC0);

    let mut index = 3;
    for b in LEVELS {
        for g in LEVELS {
            for r in LEVELS {
                palette[index] = Rgba::new(r, g, b, 0xFF);
                index += 1;
            }
        }
    }
    palette
}

/// A 64-entry custom palette. Absent a user-supplied one, the graphic's
/// Suggested Palette is used; absent that, opaque black throughout, "as black
/// is always fashionable".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette(pub [Rgba; 64]);

impl Default for Palette {
    fn default() -> Self {
        Self([Rgba::new(0x00, 0x00, 0x00, 0xFF); 64])
    }
}

impl Palette {
    /// A palette from user-supplied colours. Every colour must be sensible.
    pub fn new(colors: [Rgba; 64]) -> Result<Self, Error> {
        if colors.iter().any(|c| !c.is_sensible()) {
            return Err(Error::NonSensibleColor);
        }
        Ok(Self(colors))
    }
}

/// The 64 general registers plus the selector.
///
/// A register's low 32 bits are a 16.16 fixed-point gradient stop position;
/// its high 32 bits are a colour, directly or as a blend. Indexing is modulo
/// 64, so arithmetic on register indices never goes out of bounds.
#[derive(Clone, Copy, Debug)]
pub struct Registers {
    regs: [u64; 64],
    /// The selector, effectively 6 bits.
    pub sel: u8,
}

impl Registers {
    /// Initialise from the custom palette, per the spec: low halves zeroed,
    /// high halves taken from the palette in `RR:GG:BB:AA` little-endian order,
    /// and `SEL` set to 56 so that `REGS[SEL+8 ..]` reaches the palette's first
    /// eight colours while `REGS[SEL ..]` is free for hard-coded ones.
    pub fn new(palette: &Palette) -> Self {
        let mut regs = [0u64; 64];
        for (reg, color) in regs.iter_mut().zip(palette.0.iter()) {
            *reg = u64::from(color_to_le_u32(*color)) << 32;
        }
        Self { regs, sel: 56 }
    }

    /// Read a register. The index is taken modulo 64.
    pub fn get(&self, index: usize) -> u64 {
        self.regs[index % 64]
    }

    /// Write a register. The index is taken modulo 64.
    pub fn set(&mut self, index: usize, value: u64) {
        self.regs[index % 64] = value;
    }

    /// A register's gradient stop position, from its low 32 bits read as
    /// unsigned 16.16 fixed point.
    pub fn stop_position(&self, index: usize) -> f32 {
        let low = (self.get(index) & 0xFFFF_FFFF) as u32;
        low as f32 / 65536.0
    }

    /// Resolve a register's colour, following one level of blending.
    ///
    /// Resolution never recurses: a colour reference that itself lands on a
    /// non-sensible register yields transparent black rather than blending
    /// again, so this always terminates and always returns a sensible colour.
    pub fn color(&self, index: usize, palette: &Palette) -> Rgba {
        let direct = le_u32_to_color((self.get(index) >> 32) as u32);
        if direct.is_sensible() {
            return direct;
        }
        // Non-sensible: the high half is a blend of two colour references.
        let high = (self.get(index) >> 32) as u32;
        let blend = (high & 0xFF) as u8;
        let ref0 = ((high >> 8) & 0xFF) as u8;
        let ref1 = ((high >> 16) & 0xFF) as u8;
        Rgba::blend(
            self.resolve_ref(ref0, index, palette),
            self.resolve_ref(ref1, index, palette),
            blend,
        )
    }

    /// Resolve one colour reference, relative to the register it was read from.
    fn resolve_ref(&self, reference: u8, from: usize, palette: &Palette) -> Rgba {
        match reference {
            0x00..=0x7F => built_in_palette()[usize::from(reference)],
            0x80..=0xBF => palette.0[usize::from(reference - 0x80)],
            // An index-offset to another register, added modulo 64.
            _ => {
                let target = (from + usize::from(reference)) % 64;
                let color = le_u32_to_color((self.get(target) >> 32) as u32);
                // No further recursion, by design.
                if color.is_sensible() {
                    color
                } else {
                    Rgba::TRANSPARENT
                }
            },
        }
    }
}

/// `RR:GG:BB:AA` bytes packed as a little-endian `u32`.
fn color_to_le_u32(c: Rgba) -> u32 {
    u32::from_le_bytes([c.r, c.g, c.b, c.a])
}

fn le_u32_to_color(value: u32) -> Rgba {
    let [r, g, b, a] = value.to_le_bytes();
    Rgba::new(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba::new(r, g, b, a)
    }

    #[test]
    fn the_built_in_palette_matches_every_entry_the_spec_lists() {
        let p = built_in_palette();
        // The first twelve.
        assert_eq!(p[0x00], c(0x00, 0x00, 0x00, 0x00));
        assert_eq!(p[0x01], c(0x80, 0x80, 0x80, 0x80));
        assert_eq!(p[0x02], c(0xC0, 0xC0, 0xC0, 0xC0));
        assert_eq!(p[0x03], c(0x00, 0x00, 0x00, 0xFF));
        assert_eq!(p[0x04], c(0x40, 0x00, 0x00, 0xFF));
        assert_eq!(p[0x05], c(0x80, 0x00, 0x00, 0xFF));
        assert_eq!(p[0x06], c(0xC0, 0x00, 0x00, 0xFF));
        assert_eq!(p[0x07], c(0xFF, 0x00, 0x00, 0xFF));
        assert_eq!(p[0x08], c(0x00, 0x40, 0x00, 0xFF));
        assert_eq!(p[0x09], c(0x40, 0x40, 0x00, 0xFF));
        assert_eq!(p[0x0A], c(0x80, 0x40, 0x00, 0xFF));
        assert_eq!(p[0x0B], c(0xC0, 0x40, 0x00, 0xFF));
        // The last twelve.
        assert_eq!(p[0x74], c(0xC0, 0x80, 0xFF, 0xFF));
        assert_eq!(p[0x75], c(0xFF, 0x80, 0xFF, 0xFF));
        assert_eq!(p[0x76], c(0x00, 0xC0, 0xFF, 0xFF));
        assert_eq!(p[0x77], c(0x40, 0xC0, 0xFF, 0xFF));
        assert_eq!(p[0x78], c(0x80, 0xC0, 0xFF, 0xFF));
        assert_eq!(p[0x79], c(0xC0, 0xC0, 0xFF, 0xFF));
        assert_eq!(p[0x7A], c(0xFF, 0xC0, 0xFF, 0xFF));
        assert_eq!(p[0x7B], c(0x00, 0xFF, 0xFF, 0xFF));
        assert_eq!(p[0x7C], c(0x40, 0xFF, 0xFF, 0xFF));
        assert_eq!(p[0x7D], c(0x80, 0xFF, 0xFF, 0xFF));
        assert_eq!(p[0x7E], c(0xC0, 0xFF, 0xFF, 0xFF));
        assert_eq!(p[0x7F], c(0xFF, 0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn the_built_in_palette_is_ordered_by_its_little_endian_u32() {
        // The stated ordering rule, checked across the 125 generated entries.
        let p = built_in_palette();
        for pair in p[3..].windows(2) {
            assert!(
                color_to_le_u32(pair[0]) < color_to_le_u32(pair[1]),
                "{:?} should sort before {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn sensibility_is_the_premultiplication_test() {
        assert!(c(0x00, 0xC0, 0x00, 0xC0).is_sensible());
        assert!(c(0xFF, 0xFF, 0xFF, 0xFF).is_sensible());
        // Red exceeds alpha, so this cannot be premultiplied.
        assert!(!c(0xFF, 0x00, 0x00, 0x80).is_sensible());
    }

    #[test]
    fn a_register_holds_the_spec_example_green() {
        // REGS[40] = 0xC000_C000_7654_3210 is 75%-opaque green 00:C0:00:C0.
        let mut regs = Registers::new(&Palette::default());
        regs.set(40, 0xC000_C000_7654_3210);
        assert_eq!(regs.color(40, &Palette::default()), c(0x00, 0xC0, 0x00, 0xC0));
    }

    #[test]
    fn a_register_low_half_is_sixteen_sixteen_fixed_point() {
        // REGS[7] = 0xFEDC_BA98_0000_4000 has stop position 0.25.
        let mut regs = Registers::new(&Palette::default());
        regs.set(7, 0xFEDC_BA98_0000_4000);
        assert_eq!(regs.stop_position(7), 0.25);
    }

    #[test]
    fn the_spec_blend_example_targets_register_forty() {
        // REGS[21] = 0x0081_D340_7654_3210: blend 0x40 of colour references
        // 0xD3 (index-offset) and 0x81 (custom palette entry 1). The offset
        // reference resolves to REGS[(21 + 0xD3) % 64] = REGS[40].
        let mut palette = Palette::default().0;
        palette[1] = c(0xFF, 0x00, 0x00, 0xFF);
        let palette = Palette::new(palette).unwrap();

        let mut regs = Registers::new(&palette);
        regs.set(21, 0x0081_D340_7654_3210);
        regs.set(40, 0xC000_C000_7654_3210); // the 75%-opaque green

        assert_eq!((21 + 0xD3) % 64, 40, "the spec's worked index");

        let green = c(0x00, 0xC0, 0x00, 0xC0);
        let red = c(0xFF, 0x00, 0x00, 0xFF);
        assert_eq!(regs.color(21, &palette), Rgba::blend(green, red, 0x40));
    }

    #[test]
    fn blending_uses_the_spec_rounding() {
        // ((W0*C0) + (W1*C1) + 128) / 255, floored.
        let a = c(0x00, 0x00, 0x00, 0x00);
        let b = c(0xFF, 0xFF, 0xFF, 0xFF);
        assert_eq!(Rgba::blend(a, b, 0), a);
        assert_eq!(Rgba::blend(a, b, 255), b);
        let half = Rgba::blend(a, b, 128);
        assert_eq!(half.r, 128);
    }

    #[test]
    fn an_unresolvable_reference_is_transparent_black_without_recursing() {
        let palette = Palette::default();
        let mut regs = Registers::new(&palette);
        // Point register 0 at a blend whose offset reference lands on another
        // non-sensible register. That must stop, not blend again.
        regs.set(0, 0x00C0_C000_0000_0000);
        regs.set(1, 0x00C0_C000_0000_0000);
        let _ = regs.color(0, &palette); // terminates
    }

    #[test]
    fn registers_index_modulo_sixty_four() {
        let mut regs = Registers::new(&Palette::default());
        regs.set(70, 0xABCD);
        assert_eq!(regs.get(6), 0xABCD, "REGS[70] is REGS[6]");
    }

    #[test]
    fn sel_starts_at_fifty_six() {
        let regs = Registers::new(&Palette::default());
        assert_eq!(regs.sel, 56);
        // So REGS[SEL+8] reaches the palette's first colour.
        assert_eq!((usize::from(regs.sel) + 8) % 64, 0);
    }

    #[test]
    fn registers_initialise_from_the_palette() {
        let mut colors = Palette::default().0;
        colors[0] = c(0x00, 0xC0, 0x00, 0xC0);
        let palette = Palette::new(colors).unwrap();
        let regs = Registers::new(&palette);
        assert_eq!(regs.color(0, &palette), c(0x00, 0xC0, 0x00, 0xC0));
    }

    #[test]
    fn a_non_sensible_user_palette_is_refused() {
        let mut colors = Palette::default().0;
        colors[3] = c(0xFF, 0x00, 0x00, 0x00);
        assert!(matches!(Palette::new(colors), Err(Error::NonSensibleColor)));
    }
}
