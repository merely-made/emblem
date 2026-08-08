//! How many bytes an op occupies, without executing it.
//!
//! Jump ops jump over *ops*, not bytes: "Ops are atomic and Jumps cannot move
//! the PC into the middle of a multi-byte op." So a Jump has to walk the
//! bytecode op by op, measuring each one. That measurement is this module.
//!
//! It is deliberately separate from execution. Measuring and executing share
//! every length rule, and a version of this folded into the interpreter would
//! be a second, quietly divergent copy of the op table -- the kind of drift
//! that shows up as a jump landing one byte inside a coordinate.

use crate::num;
use crate::{Error, Result};

/// The byte length of the op beginning at `bytes[0]`.
///
/// `bytes` is the remainder of the file from the opcode onwards.
pub(crate) fn op_length(bytes: &[u8]) -> Result<usize> {
    let opcode = *bytes.first().ok_or(Error::Truncated)?;
    let low4 = usize::from(opcode & 0x0F);

    Ok(match opcode {
        // Geometry: LineTo, QuadTo, CubeTo. Coordinates per repetition differ.
        0x00..=0x2F => {
            let per_rep = match opcode >> 4 {
                0 => 2,
                1 => 4,
                _ => 6,
            };
            let (reps, header) = repetition_count(bytes, low4)?;
            header + coordinates(&bytes[header..], reps * per_rep)?
        },
        // Ellipse and Parallelogram: exactly 4 coordinates.
        0x30..=0x34 => 1 + coordinates(&bytes[1..], 4)?,
        // ClosePathMoveTo: 2 coordinates.
        0x35 => 1 + coordinates(&bytes[1..], 2)?,
        // Add to SEL: one operand byte.
        0x36 => 2,
        // NOP.
        0x37 => 1,
        // Unconditional Jump: a natural JumpCount.
        0x38 => 1 + natural_len(&bytes[1..])?,
        // FDJump: JumpCount then FeaturesNeeded, both natural.
        0x39 => {
            let a = natural_len(&bytes[1..])?;
            1 + a + natural_len(&bytes[1 + a..])?
        },
        // LODJump: JumpCount then two coordinates.
        0x3A => {
            let a = natural_len(&bytes[1..])?;
            1 + a + coordinates(&bytes[1 + a..], 2)?
        },
        // Return.
        0x3B => 1,
        // Call Untransformed: an 8-byte SegRef.
        0x3C => 1 + segref_len(&bytes[1..])?,
        // Call Transformed: an alpha byte, 6 coordinates, then a SegRef.
        0x3D => {
            let aftm = 1 + coordinates(&bytes[2..], 6)?;
            1 + aftm + segref_len(&bytes[1 + aftm..])?
        },
        // Reserved, followed by Extra Data: a NOP in this version.
        0x3E..=0x3F => 1 + extra_data_len(&bytes[1..])?,
        // Register ops: 4, 4, 8, or 8 * (LOW4 + 2) operand bytes.
        0x40..=0x5F => 5,
        0x60..=0x6F => 9,
        0x70..=0x7F => 1 + 8 * (low4 + 2),
        // Flat colour fill: opcode only.
        0x80..=0x8F => 1,
        // Linear gradient: a config byte then three float32s.
        0x90..=0x9F => 2 + 12,
        // Radial gradient: a config byte then six float32s.
        0xA0..=0xAF => 2 + 24,
        // Reserved, filling flat: Extra Data.
        0xB0..=0xBF => 1 + extra_data_len(&bytes[1..])?,
        // Reserved, a LineTo fallback: Extra Data then a coordinate pair.
        0xC0..=0xDF => {
            let extra = extra_data_len(&bytes[1..])?;
            1 + extra + coordinates(&bytes[1 + extra..], 2)?
        },
        // Reserved, a NOP: Extra Data.
        0xE0..=0xFF => 1 + extra_data_len(&bytes[1..])?,
    })
}

/// A geometry op's repetition count, and how many bytes the header took.
///
/// "If `LOW4` is non-zero, let `RepCount` equal `LOW4`. Otherwise, the opcode
/// byte is followed by a natural number and let `RepCount` equal that natural
/// number plus 16."
pub(crate) fn repetition_count(bytes: &[u8], low4: usize) -> Result<(usize, usize)> {
    if low4 != 0 {
        return Ok((low4, 1));
    }
    let (value, used) = num::natural(bytes.get(1..).ok_or(Error::Truncated)?)?;
    Ok((value as usize + 16, 1 + used))
}

/// The total byte length of `count` coordinate numbers.
fn coordinates(bytes: &[u8], count: usize) -> Result<usize> {
    let mut total = 0;
    for _ in 0..count {
        let (_, used) = num::coordinate(bytes.get(total..).ok_or(Error::Truncated)?)?;
        total += used;
    }
    Ok(total)
}

/// The byte length of one natural number.
fn natural_len(bytes: &[u8]) -> Result<usize> {
    Ok(num::natural(bytes)?.1)
}

/// A SegRef is 8 bytes, plus the segment contents when it is Inline.
///
/// Inline contents count towards the Call op's length, because the spec says
/// they "are considered part of this Call op ... when Jump ops jump over the
/// op". An Absolute SegRef's contents live elsewhere and do not.
fn segref_len(bytes: &[u8]) -> Result<usize> {
    let raw = read_u64(bytes)?;
    Ok(if is_inline(raw) {
        8 + inline_length(raw)
    } else {
        8
    })
}

/// Extra Data: a natural `EDLength` then that many bytes.
fn extra_data_len(bytes: &[u8]) -> Result<usize> {
    let (length, used) = num::natural(bytes)?;
    Ok(used + length as usize)
}

/// Read a little-endian `u64`.
pub(crate) fn read_u64(bytes: &[u8]) -> Result<u64> {
    let slice = bytes.get(..8).ok_or(Error::Truncated)?;
    Ok(u64::from_le_bytes(slice.try_into().expect("checked length")))
}

/// An Inline SegRef has an all-zero high 32 bits.
pub(crate) fn is_inline(raw: u64) -> bool {
    raw >> 32 == 0
}

/// An Inline or Absolute-Direct SegRef packs its length in bits `[8, 32)`.
pub(crate) fn inline_length(raw: u64) -> usize {
    ((raw >> 8) & 0x00FF_FFFF) as usize
}

/// The Segment Type is the low 8 bits. `0x00` means IconVG bytecode.
pub(crate) fn segment_type(raw: u64) -> u8 {
    (raw & 0xFF) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-byte coordinate. Low two bits `0b01` select the 1-byte width;
    /// `0x81` is the spec example's encoding of `+0`.
    const C1: u8 = 0x81;

    #[test]
    fn a_short_geometry_op_reads_its_count_from_the_opcode() {
        // 0x21 is one group of 6 cube coordinates, one byte each.
        let bytes = [0x21, C1, C1, C1, C1, C1, C1];
        assert_eq!(op_length(&bytes).unwrap(), 7);
    }

    #[test]
    fn a_coordinate_width_comes_from_its_own_first_byte() {
        // The same op is a different length depending on the coordinates it
        // carries: low two bits 0b00 select a 4-byte float32. Measuring by
        // opcode alone would land a jump inside a coordinate.
        let mut wide = vec![0x21];
        for _ in 0..6 {
            wide.extend_from_slice(&[0x00, 0x00, 0x80, 0x3F]);
        }
        assert_eq!(op_length(&wide).unwrap(), 1 + 24);
    }

    #[test]
    fn a_zero_low4_geometry_op_reads_a_natural_and_adds_sixteen() {
        // The spec's own example: `0x00 0x15` is 26 groups of 2 coordinates,
        // because 0x15 encodes the natural 10 and 10 + 16 = 26.
        let mut bytes = vec![0x00, 0x15];
        bytes.extend(std::iter::repeat_n(C1, 52));
        assert_eq!(op_length(&bytes).unwrap(), 2 + 52);
    }

    #[test]
    fn register_ops_have_fixed_widths() {
        assert_eq!(op_length(&[0x43, 0, 0, 0, 0]).unwrap(), 5);
        assert_eq!(op_length(&[0x57, 0, 0, 0, 0]).unwrap(), 5);
        assert_eq!(op_length(&[0x69, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(), 9);
        // 0x70 carries 8 * (LOW4 + 2) = 16 operand bytes; 0x71 carries 24.
        assert_eq!(op_length(&[0x70u8; 17]).unwrap(), 17);
        assert_eq!(op_length(&[0x71u8; 25]).unwrap(), 25);
    }

    #[test]
    fn gradient_fills_have_fixed_widths() {
        // The spec's gradient example: `91 43` then three float32s = 14 bytes.
        let linear = [0x91, 0x43, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(op_length(&linear).unwrap(), 14);
        let mut radial = [0u8; 26];
        radial[0] = 0xA1;
        radial[1] = 0x43;
        assert_eq!(op_length(&radial).unwrap(), 26);
    }

    #[test]
    fn an_inline_segref_counts_its_contents_but_an_absolute_one_does_not() {
        // The distinction a Jump depends on: inline contents are part of the
        // Call op, so jumping over the Call must clear them too.
        let mut inline = vec![0x3C];
        // Segment type 0x00, length 3, high 32 bits zero: inline.
        inline.extend_from_slice(&0x0000_0000_0000_0300u64.to_le_bytes());
        inline.extend_from_slice(&[0x37, 0x37, 0x37]);
        assert_eq!(op_length(&inline).unwrap(), 1 + 8 + 3);

        let mut absolute = vec![0x3C];
        // A non-zero high half makes it absolute; contents live elsewhere.
        absolute.extend_from_slice(&0x0000_0001_0000_0300u64.to_le_bytes());
        assert_eq!(op_length(&absolute).unwrap(), 1 + 8);
    }

    #[test]
    fn extra_data_is_measured_and_skipped() {
        // 0xE0 is a reserved NOP carrying Extra Data: a natural length then
        // that many bytes. 0x07 encodes the natural 3.
        let bytes = [0xE0, 0x07, 0xAA, 0xBB, 0xCC];
        assert_eq!(op_length(&bytes).unwrap(), 5);
    }

    #[test]
    fn a_truncated_op_is_reported_rather_than_guessed() {
        assert_eq!(op_length(&[]), Err(Error::Truncated));
        // A cube op promising 6 coordinates with only 2 present.
        assert_eq!(op_length(&[0x21, 0x00, 0x00]), Err(Error::Truncated));
    }
}
