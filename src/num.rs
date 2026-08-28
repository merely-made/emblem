// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! IconVG's three number encodings.
//!
//! Floating point numbers are always 4 bytes, little-endian IEEE 754
//! `float32`. Natural and coordinate numbers occupy 1, 2 or 4 bytes, chosen by
//! the **low two bits of the first byte**:
//!
//! | Low two bits | Width |
//! |---|---|
//! | `0x00` | 4 bytes |
//! | `0x01` | 1 byte |
//! | `0x02` | 2 bytes |
//! | `0x03` | 1 byte |
//!
//! Two of the four patterns mean one byte, which is not a mistake in this
//! implementation: the spec assigns both `0x01` and `0x03`.
//!
//! The same four bytes can be a natural number *or* a coordinate, and they do
//! not agree: `04 00 80 3F` is the natural number `266338305` and the
//! coordinate `1.000000476837158203125`. Which one it is comes from the op
//! being decoded, never from the bytes, so these are separate functions rather
//! than one function with a flag.

use crate::Error;

/// How many bytes a natural or coordinate number occupies, from its first byte.
pub(crate) fn width(first: u8) -> usize {
    match first & 0b11 {
        0x00 => 4,
        0x02 => 2,
        // Both 0x01 and 0x03 mean one byte.
        _ => 1,
    }
}

/// Decode a natural number, returning it and the bytes consumed.
///
/// 1-byte encodings carry their value in the high 7 bits, 2-byte in the high
/// 14, 4-byte in the high 30.
pub fn natural(bytes: &[u8]) -> Result<(u32, usize), Error> {
    let first = *bytes.first().ok_or(Error::Truncated)?;
    let width = width(first);
    if bytes.len() < width {
        return Err(Error::Truncated);
    }
    let value = match width {
        1 => u32::from(first >> 1),
        2 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]]) >> 2),
        _ => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) >> 2,
    };
    Ok((value, width))
}

/// Decode a coordinate number, returning it and the bytes consumed.
///
/// The shorter encodings are affine remappings of the natural decoding, which
/// is what gives them their range and granularity: 1 byte spans `[-64, +64]`
/// at integer steps, 2 bytes span `[-128, +128]` at 1/64. A 4-byte coordinate
/// is not a remapped natural at all; it is simply a `float32`.
pub fn coordinate(bytes: &[u8]) -> Result<(f32, usize), Error> {
    let first = *bytes.first().ok_or(Error::Truncated)?;
    let width = width(first);
    if bytes.len() < width {
        return Err(Error::Truncated);
    }
    let value = match width {
        1 => f32::from(first >> 1) - 64.0,
        2 => (f32::from(u16::from_le_bytes([bytes[0], bytes[1]]) >> 2) - 8192.0) / 64.0,
        _ => {
            let float = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if float.is_nan() {
                return Err(Error::NotANumber);
            }
            float
        },
    };
    Ok((value, width))
}

/// Decode a 4-byte little-endian `float32`.
///
/// `NaN` is invalid IconVG; `+Inf`, `-Inf` and `-0.0` are valid.
pub fn float32(bytes: &[u8]) -> Result<(f32, usize), Error> {
    let raw: [u8; 4] = bytes.get(..4).ok_or(Error::Truncated)?.try_into().unwrap();
    let value = f32::from_le_bytes(raw);
    if value.is_nan() {
        return Err(Error::NotANumber);
    }
    Ok((value, 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every case below is an example given in the specification itself.

    #[test]
    fn natural_numbers_match_the_spec_examples() {
        assert_eq!(natural(&[0x29]).unwrap(), (20, 1));
        assert_eq!(natural(&[0x5A, 0x83]).unwrap(), (8406, 2));
        assert_eq!(
            natural(&[0x04, 0x00, 0x80, 0x3F]).unwrap(),
            (266_338_305, 4)
        );
    }

    #[test]
    fn coordinates_match_the_spec_examples() {
        assert_eq!(coordinate(&[0x8F]).unwrap(), (7.0, 1));
        assert_eq!(coordinate(&[0x82, 0x87]).unwrap(), (7.5, 2));
        assert_eq!(coordinate(&[0x00, 0x00, 0xF0, 0x40]).unwrap(), (7.5, 4));
    }

    #[test]
    fn the_same_four_bytes_are_a_different_natural_and_coordinate() {
        // The spec uses this byte sequence for both, with different meanings.
        let bytes = [0x04, 0x00, 0x80, 0x3F];
        assert_eq!(natural(&bytes).unwrap().0, 266_338_305);
        assert_eq!(coordinate(&bytes).unwrap().0, 1.000_000_5);
    }

    #[test]
    fn both_one_byte_selectors_are_honoured() {
        // 0x01 and 0x03 in the low two bits both mean a 1-byte encoding.
        assert_eq!(width(0b0000_0001), 1);
        assert_eq!(width(0b0000_0011), 1);
        assert_eq!(width(0b0000_0010), 2);
        assert_eq!(width(0b0000_0000), 4);
    }

    #[test]
    fn a_one_byte_coordinate_spans_its_stated_range() {
        // High 7 bits minus 64, so the extremes are -64 and +63.
        assert_eq!(coordinate(&[0b0000_0001]).unwrap().0, -64.0);
        assert_eq!(coordinate(&[0b1111_1111]).unwrap().0, 63.0);
    }

    #[test]
    fn a_two_byte_coordinate_has_one_sixty_fourth_granularity() {
        let (a, _) = coordinate(&[0x82, 0x87]).unwrap();
        let (b, _) = coordinate(&[0x86, 0x87]).unwrap();
        assert_eq!(b - a, 1.0 / 64.0);
    }

    #[test]
    fn nan_is_refused_but_infinities_are_not() {
        assert!(matches!(
            coordinate(&f32::NAN.to_le_bytes()),
            Err(Error::NotANumber)
        ));
        assert!(matches!(float32(&f32::NAN.to_le_bytes()), Err(Error::NotANumber)));
        assert_eq!(float32(&f32::INFINITY.to_le_bytes()).unwrap().0, f32::INFINITY);
        assert_eq!(float32(&(-0.0f32).to_le_bytes()).unwrap().0, -0.0);
    }

    #[test]
    fn truncation_is_an_error_not_a_panic() {
        assert!(matches!(natural(&[]), Err(Error::Truncated)));
        assert!(matches!(natural(&[0x02]), Err(Error::Truncated)));
        assert!(matches!(coordinate(&[0x00, 0x00]), Err(Error::Truncated)));
        assert!(matches!(float32(&[0x00]), Err(Error::Truncated)));
    }
}
