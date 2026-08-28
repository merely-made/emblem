// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Writing IconVG's three number encodings.
//!
//! The inverse of [`crate::num`]. Width comes from the low bits of the first
//! byte: **bit 0 set** means one byte, `0b10` means two, `0b00` means four.
//!
//! One-byte encodings look like two patterns (`0b01` and `0b11`) but are not a
//! choice: bit 0 is the width flag and **bit 1 is already the value's own low
//! bit**, so the pattern falls out of the number. The specification's own
//! examples show both — natural `20` is `0x29` and coordinate `7.0` is `0x8F`.
//! There is nothing here for an encoder to decide, which is what keeps the
//! encoding a pure function of its input.
//!
//! # The four-byte coordinate carries 30 bits, not 32
//!
//! A four-byte coordinate is a `float32` read straight out of the file, but
//! its first byte must still end in `0b00` for the decoder to see it as four
//! bytes at all. Those two bits are the bottom of the mantissa, so **two bits
//! of precision are unavailable**: `0.1` cannot be stored exactly, because its
//! first byte is `0xCD` and would be misread as a one-byte coordinate.
//!
//! [`coordinate`] therefore rounds to the nearest value the format can hold,
//! which costs at most one part in 2^22 and is the only lossy step in this
//! module. Values whose low two mantissa bits are already zero — every integer
//! in range, every multiple of 1/64, `±0.0` and the infinities — survive
//! exactly. Use [`is_exact_coordinate`] to ask in advance.

use super::EncodeError;
use crate::num;

/// The largest natural number an IconVG file can carry: 30 bits.
pub const NATURAL_MAX: u32 = (1 << 30) - 1;

/// Write a natural number in the shortest encoding that holds it.
///
/// ```
/// let mut out = Vec::new();
/// emblem::encode::num::natural(20, &mut out).unwrap();
/// assert_eq!(out, [0x29]); // the specification's own example
/// ```
pub fn natural(value: u32, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if value > NATURAL_MAX {
        return Err(EncodeError::NaturalOutOfRange(value));
    }
    if value < (1 << 7) {
        out.push(((value as u8) << 1) | 0b01);
    } else if value < (1 << 14) {
        out.extend_from_slice(&(((value as u16) << 2) | 0b10).to_le_bytes());
    } else {
        out.extend_from_slice(&(value << 2).to_le_bytes());
    }
    Ok(())
}

/// How many bytes [`natural`] will write, without writing them.
pub fn natural_len(value: u32) -> Result<usize, EncodeError> {
    if value > NATURAL_MAX {
        return Err(EncodeError::NaturalOutOfRange(value));
    }
    Ok(if value < (1 << 7) {
        1
    } else if value < (1 << 14) {
        2
    } else {
        4
    })
}

/// Write a coordinate in the shortest encoding that reproduces it exactly.
///
/// Shortness is established by decoding the candidate and comparing bit
/// patterns, so a short form is used only when it round-trips exactly. See the
/// module documentation for why the comparison is on bits rather than values.
///
/// ```
/// let mut out = Vec::new();
/// emblem::encode::num::coordinate(7.0, &mut out).unwrap();
/// assert_eq!(out, [0x8F]); // one byte, the specification's example
///
/// out.clear();
/// emblem::encode::num::coordinate(7.5, &mut out).unwrap();
/// assert_eq!(out, [0x82, 0x87]); // two bytes: 7.5 is not an integer
/// ```
pub fn coordinate(value: f32, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if value.is_nan() {
        return Err(EncodeError::NotANumber);
    }

    // One byte: the high 7 bits hold `value + 64`, so integers in [-64, 63].
    if (-64.0..=63.0).contains(&value) {
        let shifted = value + 64.0;
        let candidate = [((shifted as u8) << 1) | 0b01];
        if decodes_to(&candidate, value) {
            out.extend_from_slice(&candidate);
            return Ok(());
        }
    }

    // Two bytes: the high 14 bits hold `value * 64 + 8192`, granularity 1/64.
    if (-128.0..128.0).contains(&value) {
        let scaled = value * 64.0 + 8192.0;
        if (0.0..16384.0).contains(&scaled) {
            let candidate = (((scaled as u16) << 2) | 0b10).to_le_bytes();
            if decodes_to(&candidate, value) {
                out.extend_from_slice(&candidate);
                return Ok(());
            }
        }
    }

    // Four bytes: a float32 whose low two mantissa bits must be clear, since
    // they double as the width selector. Round to the nearest such value.
    out.extend_from_slice(&quantize(value).to_le_bytes());
    Ok(())
}

/// The nearest `f32` a four-byte coordinate can hold: low two mantissa bits
/// clear, rounded to nearest, ties toward the truncated neighbour.
///
/// Deterministic by construction — every operation is an exact comparison.
pub fn quantize(value: f32) -> f32 {
    let bits = value.to_bits();
    let low = bits & !0b11;
    let high = low.wrapping_add(0b100);
    let (low_f, high_f) = (f32::from_bits(low), f32::from_bits(high));
    // The guard keeps rounding from stepping into an infinity or a NaN.
    if high_f.is_finite() && (high_f - value).abs() < (low_f - value).abs() {
        high_f
    } else {
        low_f
    }
}

/// Whether a coordinate survives encoding unchanged.
///
/// True for every value the one- and two-byte forms hold exactly, and for any
/// `f32` whose low two mantissa bits are already clear.
pub fn is_exact_coordinate(value: f32) -> bool {
    if value.is_nan() {
        return false;
    }
    let mut out = Vec::with_capacity(4);
    if coordinate(value, &mut out).is_err() {
        return false;
    }
    matches!(num::coordinate(&out), Ok((got, _)) if got.to_bits() == value.to_bits())
}

/// Whether `bytes` decode, as a coordinate, to exactly `want` — same bits, so
/// `-0.0` does not pass as `0.0`.
fn decodes_to(bytes: &[u8], want: f32) -> bool {
    matches!(num::coordinate(bytes), Ok((got, _)) if got.to_bits() == want.to_bits())
}

/// Write a 4-byte little-endian `float32`. `NaN` is refused; infinities and
/// `-0.0` are valid IconVG and pass through.
pub fn float32(value: f32, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if value.is_nan() {
        return Err(EncodeError::NotANumber);
    }
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc_nat(v: u32) -> Vec<u8> {
        let mut out = Vec::new();
        natural(v, &mut out).unwrap();
        out
    }

    fn enc_coord(v: f32) -> Vec<u8> {
        let mut out = Vec::new();
        coordinate(v, &mut out).unwrap();
        out
    }

    // ---- the specification's own examples, byte for byte --------------------

    #[test]
    fn naturals_reproduce_the_spec_example_bytes() {
        assert_eq!(enc_nat(20), [0x29]);
        assert_eq!(enc_nat(8406), [0x5A, 0x83]);
        assert_eq!(enc_nat(266_338_305), [0x04, 0x00, 0x80, 0x3F]);
    }

    #[test]
    fn coordinates_reproduce_the_spec_example_bytes() {
        assert_eq!(enc_coord(7.0), [0x8F]);
        assert_eq!(enc_coord(7.5), [0x82, 0x87]);
        // The spec's 4-byte example for 7.5 is a longer spelling of a value we
        // can write in two, so the encoder picks two. The bytes it does not
        // choose must still decode to the same number.
        assert_eq!(crate::num::coordinate(&[0x00, 0x00, 0xF0, 0x40]).unwrap().0, 7.5);
    }

    // ---- shortest form ------------------------------------------------------

    #[test]
    fn naturals_use_the_shortest_encoding_at_every_boundary() {
        assert_eq!(enc_nat(0).len(), 1);
        assert_eq!(enc_nat(127).len(), 1);
        assert_eq!(enc_nat(128).len(), 2);
        assert_eq!(enc_nat(16_383).len(), 2);
        assert_eq!(enc_nat(16_384).len(), 4);
        assert_eq!(enc_nat(NATURAL_MAX).len(), 4);
    }

    #[test]
    fn natural_len_agrees_with_what_natural_writes() {
        for v in [0, 1, 127, 128, 8406, 16_383, 16_384, 1_000_000, NATURAL_MAX] {
            assert_eq!(natural_len(v).unwrap(), enc_nat(v).len(), "for {v}");
        }
    }

    #[test]
    fn coordinates_use_the_shortest_encoding_that_is_exact() {
        assert_eq!(enc_coord(0.0).len(), 1, "an integer in range");
        assert_eq!(enc_coord(-64.0).len(), 1, "the low extreme");
        assert_eq!(enc_coord(63.0).len(), 1, "the high extreme");
        assert_eq!(enc_coord(64.0).len(), 2, "just past the 1-byte range");
        assert_eq!(enc_coord(0.25).len(), 2, "a multiple of 1/64");
        assert_eq!(enc_coord(-127.5).len(), 2);
        assert_eq!(enc_coord(0.1).len(), 4, "not a multiple of 1/64");
        assert_eq!(enc_coord(1e20).len(), 4, "far outside the short ranges");
    }

    // ---- round trips --------------------------------------------------------

    #[test]
    fn every_representable_natural_round_trips() {
        // Exhaustive across both boundaries, then sampled above them.
        let exhaustive = 0..=16_500u32;
        let sampled = (0..2000).map(|i| 16_384 + i * 500_000);
        for v in exhaustive.chain(sampled).filter(|v| *v <= NATURAL_MAX) {
            let bytes = enc_nat(v);
            let (got, used) = crate::num::natural(&bytes).unwrap();
            assert_eq!(got, v, "value for {v}");
            assert_eq!(used, bytes.len(), "width for {v}");
        }
    }

    #[test]
    fn every_one_byte_coordinate_round_trips() {
        for i in -64..=63i32 {
            let v = i as f32;
            let bytes = enc_coord(v);
            assert_eq!(bytes.len(), 1);
            assert_eq!(crate::num::coordinate(&bytes).unwrap().0, v);
        }
    }

    #[test]
    fn every_two_byte_coordinate_round_trips() {
        // The whole 1/64 lattice across the 2-byte range.
        for step in 0..16_384i32 {
            let v = (step as f32 - 8192.0) / 64.0;
            let bytes = enc_coord(v);
            let (got, used) = crate::num::coordinate(&bytes).unwrap();
            assert_eq!(got.to_bits(), v.to_bits(), "value for {v}");
            assert_eq!(used, bytes.len(), "width for {v}");
            assert!(bytes.len() <= 2, "{v} should not need four bytes");
        }
    }

    #[test]
    fn awkward_coordinates_round_trip_within_the_documented_quantization() {
        // The four-byte form loses the low two mantissa bits, so these come
        // back very slightly changed rather than exactly. The error is bounded
        // by one step at that bit position.
        for v in [
            0.1,
            -0.1,
            1e-30,
            1e30,
            f32::MIN,
            f32::MAX,
            core::f32::consts::PI,
        ] {
            let bytes = enc_coord(v);
            let (got, used) = crate::num::coordinate(&bytes).unwrap();
            assert_eq!(used, bytes.len(), "width for {v}");
            // Magnitude, not difference: for a negative float a larger bit
            // pattern is a *more* negative value, so the signed span is
            // negative and would compare against `abs` wrongly.
            let step =
                (f32::from_bits(v.to_bits() | 0b11) - f32::from_bits(v.to_bits() & !0b11)).abs();
            assert!(
                (got - v).abs() <= step,
                "{v} came back as {got}, beyond one quantization step",
            );
            assert_eq!(got.to_bits() & 0b11, 0, "the stored value must be quantized");
        }
    }

    #[test]
    fn infinities_survive_a_coordinate_round_trip_exactly() {
        // Their low mantissa bits are already clear, so nothing is lost.
        for v in [f32::INFINITY, f32::NEG_INFINITY] {
            let bytes = enc_coord(v);
            assert_eq!(crate::num::coordinate(&bytes).unwrap().0.to_bits(), v.to_bits());
        }
    }

    #[test]
    fn quantization_is_reported_honestly_in_advance() {
        // Everything the short forms hold, plus floats already aligned.
        for v in [0.0, -0.0, 7.0, -64.0, 63.0, 0.25, -127.5, 7.5, f32::INFINITY] {
            assert!(is_exact_coordinate(v), "{v} should be exact");
        }
        // And the ones that genuinely cannot be held.
        for v in [0.1, -0.1, core::f32::consts::PI, f32::MAX] {
            assert!(!is_exact_coordinate(v), "{v} should report as inexact");
        }
        assert!(!is_exact_coordinate(f32::NAN));
    }

    #[test]
    fn quantize_picks_the_nearer_neighbour_and_never_leaves_the_finite_range() {
        for v in [0.1f32, -0.1, 1e30, -1e30, core::f32::consts::PI, f32::MAX, f32::MIN] {
            let q = quantize(v);
            assert_eq!(q.to_bits() & 0b11, 0, "{v} must quantize to a storable value");
            assert!(q.is_finite(), "{v} must not round out to infinity");
            let low = f32::from_bits(v.to_bits() & !0b11);
            let high = f32::from_bits((v.to_bits() & !0b11).wrapping_add(0b100));
            let nearer = (high - v).abs() < (low - v).abs() && high.is_finite();
            let best = if nearer { high } else { low };
            assert_eq!(q.to_bits(), best.to_bits(), "for {v}");
        }
        // Already-aligned values are left alone.
        for v in [0.0f32, -0.0, 7.5, f32::INFINITY] {
            assert_eq!(quantize(v).to_bits(), v.to_bits(), "for {v}");
        }
    }

    #[test]
    fn negative_zero_survives_instead_of_becoming_positive_zero() {
        // The reason the shortest-form check compares bits: +0.0 would pass an
        // `==` test and silently replace -0.0.
        let bytes = enc_coord(-0.0);
        assert_eq!(bytes.len(), 4, "-0.0 needs the long form to survive");
        let got = crate::num::coordinate(&bytes).unwrap().0;
        assert_eq!(got.to_bits(), (-0.0f32).to_bits());
        assert!(got.is_sign_negative());

        // And +0.0 is still cheap.
        assert_eq!(enc_coord(0.0).len(), 1);
    }

    // ---- refusals -----------------------------------------------------------

    #[test]
    fn out_of_range_naturals_and_nan_are_refused() {
        let mut out = Vec::new();
        assert_eq!(
            natural(NATURAL_MAX + 1, &mut out),
            Err(EncodeError::NaturalOutOfRange(NATURAL_MAX + 1))
        );
        assert_eq!(coordinate(f32::NAN, &mut out), Err(EncodeError::NotANumber));
        assert_eq!(float32(f32::NAN, &mut out), Err(EncodeError::NotANumber));
        assert!(out.is_empty(), "a refused write must leave the buffer alone");
    }

    #[test]
    fn float32_keeps_infinities_and_negative_zero() {
        for v in [f32::INFINITY, f32::NEG_INFINITY, -0.0, 0.0] {
            let mut out = Vec::new();
            float32(v, &mut out).unwrap();
            assert_eq!(out.len(), 4);
            assert_eq!(crate::num::float32(&out).unwrap().0.to_bits(), v.to_bits());
        }
    }

    // ---- determinism --------------------------------------------------------

    #[test]
    fn encoding_is_deterministic_across_repeated_runs() {
        let naturals: Vec<u32> = (0..3000).map(|i| i * 37).collect();
        let coords: Vec<f32> = (0..3000).map(|i| (i as f32 - 1500.0) / 64.0).collect();

        let once = {
            let mut out = Vec::new();
            for &v in &naturals {
                natural(v, &mut out).unwrap();
            }
            for &v in &coords {
                coordinate(v, &mut out).unwrap();
            }
            out
        };
        for _ in 0..8 {
            let again = {
                let mut out = Vec::new();
                for &v in &naturals {
                    natural(v, &mut out).unwrap();
                }
                for &v in &coords {
                    coordinate(v, &mut out).unwrap();
                }
                out
            };
            assert_eq!(once, again, "the same input must give the same bytes");
        }
    }

    #[test]
    fn the_two_one_byte_patterns_are_the_values_own_low_bit() {
        // Not two spellings to choose between: bit 0 is the width flag and
        // bit 1 is the number's low bit, so the pattern is forced by the
        // value. The spec shows both — natural 20 is 0x29, coordinate 7.0 is
        // 0x8F. An encoder with a choice here would not be a pure function.
        for v in 0..=127u32 {
            let byte = enc_nat(v)[0];
            assert_eq!(byte & 0b1, 0b1, "natural {v} must set the width flag");
            assert_eq!(
                u32::from(byte >> 1) & 0b1,
                v & 0b1,
                "bit 1 of {v} must be the value's low bit",
            );
        }
        assert_eq!(enc_nat(20)[0], 0x29, "the spec's even example");
        assert_eq!(enc_nat(1)[0] & 0b11, 0b11, "an odd value uses the other pattern");
        assert_eq!(enc_coord(7.0)[0], 0x8F, "the spec's odd-patterned coordinate");
    }

    #[test]
    fn width_selectors_are_correct_for_the_longer_forms() {
        // Two-byte forms end 0b10, four-byte forms 0b00, or the decoder reads
        // them as something shorter. This is what the 4-byte quantization is
        // in aid of.
        assert_eq!(enc_nat(8406)[0] & 0b11, 0b10);
        assert_eq!(enc_nat(266_338_305)[0] & 0b11, 0b00);
        assert_eq!(enc_coord(7.5)[0] & 0b11, 0b10);
        assert_eq!(enc_coord(0.1)[0] & 0b11, 0b00);
    }
}
