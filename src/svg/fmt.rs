// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Fixed-point number text that never changes between toolchains.
//!
//! Rust's `{}` for floats is shortest-round-trip, which is correct but not a
//! promise that two compilers print the same digits forever. SVG text is meant
//! to be content-addressed like the `.ivg` it came from, so numbers go through
//! this instead: scale in `f64`, round half away from zero, trim trailing
//! zeros, never print `-0`. Only IEEE `+ - * /` and truncating casts are used.

use super::SvgError;

/// Fractional digits for path coordinates, stop offsets and opacities.
pub(crate) const COORD: u32 = 4;
/// Fractional digits for gradient points, matrices and clip geometry.
pub(crate) const GRADIENT: u32 = 6;

/// Scaled magnitudes at or above this do not fit the `u128` the digits are
/// cut from. With four digits that is about `1.7e34`, far past any ViewBox.
const LIMIT: f64 = 1.7e38;

/// Append `value` with at most `digits` fractional digits.
pub(crate) fn number(out: &mut String, value: f64, digits: u32) -> Result<(), SvgError> {
    if !value.is_finite() {
        return Err(SvgError::NonFinite);
    }
    let scale = 10u128.pow(digits);
    let scaled = value.abs() * scale as f64;
    if scaled >= LIMIT {
        return Err(SvgError::OutOfRange);
    }
    let whole = scaled as u128;
    let rounded = if scaled - whole as f64 >= 0.5 {
        whole + 1
    } else {
        whole
    };
    if rounded == 0 {
        out.push('0');
        return Ok(());
    }
    if value < 0.0 {
        out.push('-');
    }
    let (int, mut frac) = (rounded / scale, rounded % scale);
    push_u128(out, int);
    if frac != 0 {
        let mut width = digits;
        while frac % 10 == 0 {
            frac /= 10;
            width -= 1;
        }
        out.push('.');
        let mut text = String::new();
        push_u128(&mut text, frac);
        for _ in text.len()..width as usize {
            out.push('0');
        }
        out.push_str(&text);
    }
    Ok(())
}

fn push_u128(out: &mut String, mut value: u128) {
    let mut digits = [0u8; 40];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in &digits[at..] {
        out.push(char::from(digit));
    }
}

/// Append `#rrggbb`.
pub(crate) fn hex(out: &mut String, [r, g, b]: [u8; 3]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    out.push('#');
    for channel in [r, g, b] {
        out.push(char::from(DIGITS[usize::from(channel >> 4)]));
        out.push(char::from(DIGITS[usize::from(channel & 0x0f)]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(value: f64, digits: u32) -> Result<String, SvgError> {
        let mut out = String::new();
        number(&mut out, value, digits).map(|()| out)
    }

    #[test]
    fn numbers_print_the_same_way_every_time() {
        let table: &[(f64, u32, &str)] = &[
            (0.0, COORD, "0"),
            (-0.0, COORD, "0"),
            (-0.04, 1, "0"),
            (0.25, 1, "0.3"),
            (-0.25, 1, "-0.3"),
            (0.5, COORD, "0.5"),
            (-0.5, COORD, "-0.5"),
            (1.0 / 3.0, COORD, "0.3333"),
            (2.0 / 3.0, COORD, "0.6667"),
            (1.0 / 3.0, GRADIENT, "0.333333"),
            (24.0, COORD, "24"),
            (-24.0, COORD, "-24"),
            (10.05, COORD, "10.05"),
            (0.0001, COORD, "0.0001"),
            (123_456_789.125, COORD, "123456789.125"),
            (1_152_921_504_606_846_976.0, COORD, "1152921504606846976"),
        ];
        for &(value, digits, want) in table {
            assert_eq!(
                show(value, digits).as_deref(),
                Ok(want),
                "{value} at {digits}"
            );
        }
    }

    #[test]
    fn numbers_that_cannot_be_written_are_errors() {
        assert_eq!(show(f64::NAN, COORD), Err(SvgError::NonFinite));
        assert_eq!(show(f64::INFINITY, COORD), Err(SvgError::NonFinite));
        assert_eq!(show(-1.0e35, COORD), Err(SvgError::OutOfRange));
    }

    #[test]
    fn colours_print_as_six_lowercase_hex_digits() {
        let mut out = String::new();
        hex(&mut out, [0x0a, 0xff, 0x80]);
        assert_eq!(out, "#0aff80");
    }
}
