// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Writing colours, colour references and registers.
//!
//! The inverse of [`crate::color`]. Three things live here, in rising order of
//! how much of the format they touch:
//!
//! 1. **Colours** as `RR:GG:BB:AA` bytes, which is how the suggested palette
//!    is stored and how a register's high half holds a literal colour.
//! 2. **Colour references**, one byte, in three bands: the built-in palette,
//!    the custom palette, and an offset to another register.
//! 3. **Registers**, 64 bits: a colour or a blend in the high half, a
//!    gradient stop position as unsigned 16.16 fixed point in the low half.
//!
//! # Why [`ColorSource::Custom`] is the interesting one
//!
//! A literal colour is baked into the file; a custom-palette reference is
//! resolved at *decode* time against whatever palette the caller supplies. A
//! generator that emits literal colours produces files that can never be
//! re-themed, which throws away the reason to use this format at all. The API
//! is arranged so the palette-indexed spelling is the ordinary one.

use super::EncodeError;
use crate::color::Rgba;

/// Where a colour comes from when the file is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSource {
    /// A literal colour, fixed in the file. It cannot be re-themed.
    Literal(Rgba),
    /// Custom-palette entry `0..=63`, resolved against the palette supplied to
    /// [`execute`](crate::execute). This is the spelling that lets one file
    /// wear every theme.
    Custom(u8),
    /// Built-in-palette entry `0..=127`, fixed by the specification.
    BuiltIn(u8),
    /// An offset from the register being read, added modulo 64. The encodable
    /// offsets are `0x40..=0x7F`, which the reference byte carries as
    /// `0xC0..=0xFF`.
    RegisterOffset(u8),
}

/// Write a colour as `RR:GG:BB:AA`.
pub fn rgba(color: Rgba, out: &mut Vec<u8>) {
    out.extend_from_slice(&[color.r, color.g, color.b, color.a]);
}

/// A colour packed as a little-endian `u32`, which is how a register's high
/// half holds one.
pub fn rgba_to_le_u32(color: Rgba) -> u32 {
    u32::from_le_bytes([color.r, color.g, color.b, color.a])
}

/// Write one colour reference byte.
///
/// The bands are `0x00..=0x7F` built-in, `0x80..=0xBF` custom, `0xC0..=0xFF`
/// register offset. [`ColorSource::Literal`] has no one-byte spelling and is
/// refused here; a literal belongs in a register's high half, written with
/// [`register`].
pub fn color_reference(source: ColorSource, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    out.push(color_reference_byte(source)?);
    Ok(())
}

/// The reference byte for a source, without writing it.
pub fn color_reference_byte(source: ColorSource) -> Result<u8, EncodeError> {
    match source {
        ColorSource::BuiltIn(i) if i <= 0x7F => Ok(i),
        ColorSource::BuiltIn(i) => Err(EncodeError::BuiltInIndexOutOfRange(i)),
        ColorSource::Custom(i) if i <= 0x3F => Ok(0x80 | i),
        ColorSource::Custom(i) => Err(EncodeError::PaletteIndexOutOfRange(i)),
        // The reference byte is the offset itself; only the top band is an
        // offset, so the encodable offsets are 0x40..=0x7F.
        ColorSource::RegisterOffset(o) => Ok(o | 0xC0),
        // A literal has no one-byte form.
        ColorSource::Literal(_) => Err(EncodeError::LiteralHasNoReference),
    }
}

/// Assemble a register holding a literal colour and a gradient stop position.
///
/// `stop` is unsigned 16.16 fixed point in the low half, so it must lie in
/// `[0, 1)`. The colour must be *sensible* (`R <= A`, `G <= A`, `B <= A`);
/// a non-sensible high half is how the format spells a blend instead, so
/// writing one here by accident would silently change what the register means.
/// Use [`blend_register`] for that deliberately.
pub fn register(color: Rgba, stop: f32, out: &mut Vec<u8>) -> Result<u64, EncodeError> {
    let value = register_value(color, stop)?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(value)
}

/// The register value for a literal colour and stop position, without writing.
pub fn register_value(color: Rgba, stop: f32) -> Result<u64, EncodeError> {
    Ok((u64::from(rgba_to_le_u32(color)) << 32) | u64::from(fixed_16_16(stop)?))
}

/// Assemble a register whose high half is a *blend* of two colour references.
///
/// The blend weight runs `0` (all of `zero`) to `255` (all of `one`). The high
/// half is laid out `ref1:ref0:blend` from the top, and is recognised as a
/// blend precisely because it is not a sensible colour — so this refuses to
/// produce a high half that would read back as a literal colour instead.
pub fn blend_register(
    zero: ColorSource,
    one: ColorSource,
    blend: u8,
    stop: f32,
    out: &mut Vec<u8>,
) -> Result<u64, EncodeError> {
    let value = blend_register_value(zero, one, blend, stop)?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(value)
}

/// The register value for a blend, without writing it.
pub fn blend_register_value(
    zero: ColorSource,
    one: ColorSource,
    blend: u8,
    stop: f32,
) -> Result<u64, EncodeError> {
    let ref0 = color_reference_byte(zero)?;
    let ref1 = color_reference_byte(one)?;
    let high = (u32::from(ref1) << 16) | (u32::from(ref0) << 8) | u32::from(blend);

    // Nonsense is the tag. If these bytes happen to be a sensible colour, the
    // decoder will read a literal and the blend is lost, so refuse rather than
    // emit something that silently means the wrong thing.
    if le_u32_is_sensible(high) {
        return Err(EncodeError::BlendReadsAsColor { packed: high });
    }

    Ok((u64::from(high) << 32) | u64::from(fixed_16_16(stop)?))
}

/// Whether a packed high half would be read as a literal colour rather than a
/// blend.
fn le_u32_is_sensible(value: u32) -> bool {
    let [r, g, b, a] = value.to_le_bytes();
    Rgba::new(r, g, b, a).is_sensible()
}

/// `[0, 1)` as unsigned 16.16 fixed point.
fn fixed_16_16(stop: f32) -> Result<u32, EncodeError> {
    if !(0.0..1.0).contains(&stop) {
        return Err(EncodeError::StopPositionOutOfRange(stop));
    }
    // Exact: the product is below 65536 and truncation is the inverse of the
    // decoder's `low as f32 / 65536.0`.
    Ok((stop * 65536.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{Palette, Registers, built_in_palette};

    fn c(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba::new(r, g, b, a)
    }

    #[test]
    fn a_colour_writes_as_rgba_bytes() {
        let mut out = Vec::new();
        rgba(c(0x00, 0xC0, 0x00, 0xC0), &mut out);
        assert_eq!(out, [0x00, 0xC0, 0x00, 0xC0]);
    }

    #[test]
    fn colour_references_land_in_the_right_bands() {
        assert_eq!(color_reference_byte(ColorSource::BuiltIn(0)).unwrap(), 0x00);
        assert_eq!(color_reference_byte(ColorSource::BuiltIn(0x7F)).unwrap(), 0x7F);
        assert_eq!(color_reference_byte(ColorSource::Custom(0)).unwrap(), 0x80);
        assert_eq!(color_reference_byte(ColorSource::Custom(0x3F)).unwrap(), 0xBF);
        assert_eq!(
            color_reference_byte(ColorSource::RegisterOffset(0x53)).unwrap(),
            0xD3,
            "the spec's worked blend uses reference 0xD3",
        );
    }

    #[test]
    fn a_literal_has_no_reference_spelling() {
        assert_eq!(
            color_reference_byte(ColorSource::Literal(c(1, 2, 3, 4))),
            Err(EncodeError::LiteralHasNoReference),
        );
    }

    #[test]
    fn out_of_band_indices_are_refused() {
        assert_eq!(
            color_reference_byte(ColorSource::Custom(64)),
            Err(EncodeError::PaletteIndexOutOfRange(64))
        );
        assert_eq!(
            color_reference_byte(ColorSource::BuiltIn(128)),
            Err(EncodeError::BuiltInIndexOutOfRange(128))
        );
    }

    // ---- against the decoder ------------------------------------------------

    #[test]
    fn every_custom_reference_resolves_to_that_palette_entry() {
        // The property the whole theming story rests on: the file names a slot,
        // and the caller's palette decides the colour.
        let mut colors = Palette::default().0;
        for (i, slot) in colors.iter_mut().enumerate() {
            *slot = c(i as u8, 0x00, 0x00, 0xFF);
        }
        let palette = Palette::new(colors).unwrap();
        let mut regs = Registers::new(&palette);

        for i in 0..64u8 {
            let value = blend_register_value(
                ColorSource::Custom(i),
                ColorSource::Custom(i),
                0,
                0.0,
            )
            .unwrap();
            regs.set(0, value);
            assert_eq!(
                regs.color(0, &palette),
                palette.0[usize::from(i)],
                "custom slot {i}",
            );
        }
    }

    #[test]
    fn one_file_wears_two_palettes() {
        // The same register bytes, decoded against different palettes, give
        // different colours. This is the re-theming property in miniature.
        let value =
            blend_register_value(ColorSource::Custom(3), ColorSource::Custom(3), 0, 0.0).unwrap();

        let make = |color: Rgba| {
            let mut colors = Palette::default().0;
            colors[3] = color;
            Palette::new(colors).unwrap()
        };
        let light = make(c(0xFF, 0xFF, 0xFF, 0xFF));
        let dark = make(c(0x10, 0x10, 0x10, 0xFF));

        let mut regs = Registers::new(&light);
        regs.set(0, value);
        assert_eq!(regs.color(0, &light), c(0xFF, 0xFF, 0xFF, 0xFF));

        let mut regs = Registers::new(&dark);
        regs.set(0, value);
        assert_eq!(regs.color(0, &dark), c(0x10, 0x10, 0x10, 0xFF));
    }

    #[test]
    fn a_built_in_reference_resolves_to_the_built_in_palette() {
        let palette = Palette::default();
        let mut regs = Registers::new(&palette);
        // Index 0 is excluded deliberately: see the collision test below.
        for i in [1u8, 2, 3, 0x07, 0x40, 0x7F] {
            let value =
                blend_register_value(ColorSource::BuiltIn(i), ColorSource::BuiltIn(i), 0, 0.0)
                    .unwrap();
            regs.set(0, value);
            assert_eq!(regs.color(0, &palette), built_in_palette()[usize::from(i)]);
        }
    }

    #[test]
    fn a_blend_that_would_read_back_as_a_colour_is_refused() {
        // Built-in 0 twice at weight 0 packs to 00:00:00:00 — a sensible
        // transparent black, so the decoder would see a literal colour and the
        // blend would be silently lost. The encoder must not emit it.
        let err = blend_register_value(
            ColorSource::BuiltIn(0),
            ColorSource::BuiltIn(0),
            0,
            0.0,
        )
        .unwrap_err();
        assert_eq!(err, EncodeError::BlendReadsAsColor { packed: 0 });

        // A non-zero weight breaks the collision, and so does any reference
        // outside the very bottom of the built-in band.
        assert!(
            blend_register_value(ColorSource::BuiltIn(0), ColorSource::BuiltIn(0), 1, 0.0).is_ok()
        );
        assert!(
            blend_register_value(ColorSource::Custom(0), ColorSource::Custom(0), 0, 0.0).is_ok()
        );
    }

    #[test]
    fn the_refusal_fires_exactly_when_the_decoder_would_misread() {
        // Sweep the whole built-in band and check the encoder's judgement
        // agrees with what the decoder actually does with those bytes.
        let palette = Palette::default();
        let mut regs = Registers::new(&palette);
        for i in 0..=0x7Fu8 {
            for blend in [0u8, 1, 0x80, 0xFF] {
                let built_in = ColorSource::BuiltIn(i);
                let attempt = blend_register_value(built_in, built_in, blend, 0.0);
                let packed = (u32::from(i) << 16) | (u32::from(i) << 8) | u32::from(blend);
                let misread = le_u32_is_sensible(packed);
                assert_eq!(attempt.is_err(), misread, "built-in {i}, blend {blend}");
                if let Ok(value) = attempt {
                    regs.set(0, value);
                    // It really is treated as a blend, not a literal.
                    assert_eq!(
                        regs.color(0, &palette),
                        Rgba::blend(
                            built_in_palette()[usize::from(i)],
                            built_in_palette()[usize::from(i)],
                            blend,
                        ),
                    );
                }
            }
        }
    }

    #[test]
    fn a_literal_register_round_trips_through_the_decoder() {
        let palette = Palette::default();
        let mut regs = Registers::new(&palette);
        for color in [
            c(0x00, 0xC0, 0x00, 0xC0),
            c(0xFF, 0xFF, 0xFF, 0xFF),
            c(0x00, 0x00, 0x00, 0x00),
            c(0x40, 0x20, 0x10, 0x80),
        ] {
            let value = register_value(color, 0.0).unwrap();
            regs.set(0, value);
            assert_eq!(regs.color(0, &palette), color);
        }
    }

    #[test]
    fn the_spec_register_example_is_reproduced_exactly() {
        // REGS[40] = 0xC000_C000_7654_3210 is 75%-opaque green with a stop
        // position in the low half. We reproduce the high half; the low half
        // here is the spec's arbitrary bit pattern, so compare the top only.
        let value = register_value(c(0x00, 0xC0, 0x00, 0xC0), 0.0).unwrap();
        assert_eq!(value >> 32, 0xC000_C000);
    }

    #[test]
    fn the_spec_blend_example_is_reproduced_exactly() {
        // REGS[21] = 0x0081_D340_...: blend 0x40 of references 0xD3 and 0x81.
        let value = blend_register_value(
            ColorSource::RegisterOffset(0x53), // 0xD3
            ColorSource::Custom(1),            // 0x81
            0x40,
            0.0,
        )
        .unwrap();
        assert_eq!(value >> 32, 0x0081_D340);
    }

    #[test]
    fn a_blend_decodes_to_the_blend_of_its_two_references() {
        let mut colors = Palette::default().0;
        colors[1] = c(0xFF, 0x00, 0x00, 0xFF);
        let palette = Palette::new(colors).unwrap();

        let mut regs = Registers::new(&palette);
        let green = c(0x00, 0xC0, 0x00, 0xC0);
        regs.set(40, register_value(green, 0.0).unwrap());
        regs.set(
            21,
            blend_register_value(
                ColorSource::RegisterOffset(0x53),
                ColorSource::Custom(1),
                0x40,
                0.0,
            )
            .unwrap(),
        );

        let red = c(0xFF, 0x00, 0x00, 0xFF);
        assert_eq!(regs.color(21, &palette), Rgba::blend(green, red, 0x40));
    }

    // ---- stop positions -----------------------------------------------------

    #[test]
    fn stop_positions_round_trip_through_the_decoder() {
        let palette = Palette::default();
        let mut regs = Registers::new(&palette);
        for stop in [0.0, 0.25, 0.5, 0.75, 0.125, 0.9999847] {
            let value = register_value(c(0, 0, 0, 0xFF), stop).unwrap();
            regs.set(7, value);
            let got = regs.stop_position(7);
            assert!(
                (got - stop).abs() <= 1.0 / 65536.0,
                "stop {stop} came back as {got}",
            );
        }
    }

    #[test]
    fn the_spec_stop_position_example_is_reproduced() {
        // REGS[7] low half 0x0000_4000 is 0.25.
        let value = register_value(c(0, 0, 0, 0xFF), 0.25).unwrap();
        assert_eq!(value & 0xFFFF_FFFF, 0x0000_4000);
    }

    #[test]
    fn stop_positions_outside_the_unit_interval_are_refused() {
        assert!(matches!(
            register_value(c(0, 0, 0, 0xFF), 1.0),
            Err(EncodeError::StopPositionOutOfRange(_))
        ));
        assert!(matches!(
            register_value(c(0, 0, 0, 0xFF), -0.1),
            Err(EncodeError::StopPositionOutOfRange(_))
        ));
    }

    // ---- determinism --------------------------------------------------------

    #[test]
    fn colour_encoding_is_deterministic_across_repeated_runs() {
        let build = || {
            let mut out = Vec::new();
            for i in 0..64u8 {
                color_reference(ColorSource::Custom(i), &mut out).unwrap();
                rgba(c(i, i.wrapping_mul(3), i.wrapping_mul(7), 0xFF), &mut out);
                register(c(i, i, i, 0xFF), f32::from(i) / 64.0, &mut out).unwrap();
            }
            out
        };
        let once = build();
        for _ in 0..8 {
            assert_eq!(once, build());
        }
    }
}
