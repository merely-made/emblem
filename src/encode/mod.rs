// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Writing IconVG.
//!
//! The inverse of the decoder, in the same layers: [`num`] writes the number
//! encodings, [`color`] writes colours, references and registers. Whole-file
//! assembly arrives in a later phase; this layer is what everything above it
//! is built from.
//!
//! # Determinism
//!
//! **The same input always produces the same bytes**, on every machine and in
//! every process. This is a hard requirement rather than a nicety: the
//! motivating consumer derives icons from content addresses, so two peers must
//! independently produce identical files, and the files may themselves be
//! content-addressed. Nothing here consults a hash map, the clock, the
//! allocator, or any float operation whose result is not exactly specified.
//!
//! # Shortest form, verified rather than reasoned
//!
//! Natural and coordinate numbers have 1-, 2- and 4-byte encodings, and the
//! encoder always emits the shortest one that survives a round trip. It
//! establishes that by *decoding what it just wrote* with the real decoder and
//! comparing bit patterns, rather than by reasoning about ranges and
//! granularity. That costs a few nanoseconds and buys the property outright:
//! a short form is emitted only when it is exactly right.
//!
//! Bit-pattern comparison is deliberately stricter than `==`. It sends `-0.0`
//! to the 4-byte form, because the 1-byte form would decode to `+0.0`, which
//! compares equal while being a different number. Geometrically the two are
//! interchangeable; making the encoder assume so would put a documented
//! exception in the round-trip property, and the property is worth more than
//! the three bytes.
//!
//! There is exactly one lossy step in this module, and it is the format's
//! rather than ours: a four-byte coordinate's first byte doubles as the width
//! selector, so the low two mantissa bits cannot be stored and a value like
//! `0.1` is rounded to the nearest one that can be. Everything the one- and
//! two-byte forms hold — integers in range, multiples of 1/64 — is exact, as
//! is any float already aligned. See [`num`] for the detail and
//! [`num::is_exact_coordinate`] to ask about a particular value.

pub mod color;
pub mod num;
pub mod style;
pub mod writer;

pub use writer::{CubeSegment, Point, QuadSegment, Writer};

/// What can go wrong writing a graphic.
///
/// Separate from the decoder's [`Error`](crate::Error) because the failures
/// have nothing in common: a decoder meets malformed input, an encoder meets
/// a value it cannot represent. Keeping them apart also leaves the published
/// decode API untouched.
// No `Eq`: one variant carries the offending `f32`, which is worth keeping in
// the error even though floats are only `PartialEq`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum EncodeError {
    /// A natural number above the 30-bit maximum, `1_073_741_823`.
    NaturalOutOfRange(u32),
    /// A coordinate that is `NaN`. Infinities are representable; `NaN` is not
    /// valid IconVG, and the decoder refuses it.
    NotANumber,
    /// A custom-palette index above 63.
    PaletteIndexOutOfRange(u8),
    /// A built-in-palette index above 127.
    BuiltInIndexOutOfRange(u8),
    /// A gradient stop position outside `[0, 1)`, which the 16.16 fixed-point
    /// low half of a register cannot hold.
    StopPositionOutOfRange(f32),
    /// A blend whose packed bytes form a *sensible* colour, and so would be
    /// read back as a literal colour rather than a blend.
    ///
    /// The format tags blends by nonsense: a register's high half holds a
    /// colour when `R <= A`, `G <= A` and `B <= A`, and a blend when it does
    /// not. A blend of two low built-in indices at weight zero packs to
    /// `00:00:00:00`, which is a perfectly sensible transparent black, so the
    /// intent is unrecoverable. Reference a register or the custom palette
    /// instead, or choose a non-zero weight.
    BlendReadsAsColor {
        /// The packed high half that collided.
        packed: u32,
    },
    /// A literal colour was used where a one-byte colour *reference* is
    /// required. Literals have no reference spelling; put one in a register's
    /// high half instead, or name a palette entry.
    LiteralHasNoReference,
    /// A register slot above 15. Slots are the low four bits of an opcode.
    SlotOutOfRange(u8),
    /// A multi-register write of fewer than 2 or more than
    /// [`style::MAX_REGISTERS_PER_OP`] registers.
    RegisterCountOutOfRange(usize),
    /// A gradient with fewer than 2 or more than
    /// [`style::MAX_GRADIENT_STOPS`] stops. The encoding reserves the value
    /// that would mean 65, so the ceiling is 64.
    GradientStopsOutOfRange(usize),
    /// A gradient transform of the wrong length: three values for a linear
    /// gradient, six for a radial one.
    GradientMatrixWrongLength {
        /// How many the gradient kind requires.
        expected: usize,
        /// How many were supplied.
        found: usize,
    },
    /// A geometry op with no repetitions. Every run needs at least one.
    EmptyRun,
    /// An ellipse with a quarter count outside 1 to 4.
    EllipseQuartersOutOfRange(u8),
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NaturalOutOfRange(v) => {
                write!(f, "natural number {v} exceeds the 30-bit maximum 1073741823")
            },
            Self::NotANumber => f.write_str("NaN is not a valid IconVG number"),
            Self::PaletteIndexOutOfRange(i) => {
                write!(f, "custom palette index {i} exceeds 63")
            },
            Self::BuiltInIndexOutOfRange(i) => {
                write!(f, "built-in palette index {i} exceeds 127")
            },
            Self::StopPositionOutOfRange(p) => {
                write!(f, "gradient stop position {p} lies outside [0, 1)")
            },
            Self::LiteralHasNoReference => f.write_str(
                "a literal colour has no one-byte reference spelling",
            ),
            Self::SlotOutOfRange(s) => write!(f, "register slot {s} exceeds 15"),
            Self::RegisterCountOutOfRange(n) => write!(
                f,
                "a multi-register write takes 2 to {} registers, not {n}",
                style::MAX_REGISTERS_PER_OP,
            ),
            Self::GradientStopsOutOfRange(n) => write!(
                f,
                "a gradient takes 2 to {} stops, not {n}",
                style::MAX_GRADIENT_STOPS,
            ),
            Self::GradientMatrixWrongLength { expected, found } => write!(
                f,
                "this gradient kind needs {expected} transform values, not {found}",
            ),
            Self::EmptyRun => f.write_str("a geometry op needs at least one repetition"),
            Self::EllipseQuartersOutOfRange(q) => {
                write!(f, "an ellipse takes 1 to 4 quarters, not {q}")
            },
            Self::BlendReadsAsColor { packed } => write!(
                f,
                "blend packs to {packed:#010X}, which is a sensible colour and \
                 would be read back as a literal rather than a blend",
            ),
        }
    }
}

impl core::error::Error for EncodeError {}
