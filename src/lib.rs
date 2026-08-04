//! # iconvg
//!
//! A pure-Rust implementation of [IconVG](https://github.com/nigeltao/iconvg),
//! a compact binary format for simple vector graphics: icons, logos, glyphs
//! and emoji. It is far simpler than SVG on purpose, with no text, multimedia,
//! interactivity, scripting, animation or DOM, and it is a presentation format
//! rather than an authoring one: strokes are already flattened to filled paths
//! before they reach a file.
//!
//! ## Status: foundation only, and the format is experimental
//!
//! **The format itself carries the warning "THIS FORMAT IS EXPERIMENTAL AND
//! SUBJECT TO INCOMPATIBLE CHANGES."** This crate is therefore pinned to a
//! specific revision of the specification (recorded in the README) and is not
//! published to crates.io. Taking the `iconvg` name on a registry would claim
//! a stability neither the format nor this crate can currently offer.
//!
//! What is implemented, faithfully and tested against the specification's own
//! worked examples:
//!
//! - [`num`] — the natural, coordinate and `float32` encodings;
//! - [`color`] — alpha-premultiplied colour, the 128-entry built-in palette,
//!   the 64-entry custom palette, the register file, and colour-reference
//!   resolution including blends;
//! - [`meta`] — the magic identifier, metadata chunks, the ViewBox and the
//!   suggested palette.
//!
//! What is **not** implemented yet: the bytecode virtual machine. IconVG is a
//! register-based VM with geometry, fill, register and control-flow ops
//! (including jumps, calls and feature detection), and that layer is the
//! remaining work. Nothing here pretends to rasterize a graphic.
//!
//! ## Why alpha-premultiplied is worth knowing about
//!
//! Colour is premultiplied throughout, gradient interpolation included, which
//! differs from SVG. Halfway between opaque bright red and transparent black is
//! a 50%-opaque *bright* red rather than a 50%-opaque dark red: hue and
//! saturation survive and only alpha falls.
//!
//! A consequence worth not tripping over: a colour 4-tuple that is *not*
//! sensible under premultiplication (`R <= A`, `G <= A`, `B <= A`) is not
//! necessarily an error. In a register, non-sensibility is the tag that says
//! "this is a blend of two colour references", so the nonsense carries meaning.

#![forbid(unsafe_code)]

pub mod color;
pub mod meta;
pub mod num;

pub use color::{Palette, Registers, Rgba, built_in_palette};
pub use meta::{MAGIC, Metadata, ViewBox};

/// What can go wrong reading an IconVG graphic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file does not begin with `"\x8aIVG"`.
    BadMagic,
    /// The file ended in the middle of something.
    Truncated,
    /// A `NaN` appeared where IconVG forbids it. Infinities are permitted.
    NotANumber,
    /// A ViewBox was inverted or infinite.
    InvalidViewBox,
    /// A palette declared more than 64 entries.
    InvalidPalette,
    /// A colour was not sensible under alpha-premultiplication, somewhere that
    /// requires one.
    NonSensibleColor,
    /// Metadata chunks were out of order or repeated a MID.
    MetadataOrder,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::BadMagic => "not an IconVG file (bad magic identifier)",
            Self::Truncated => "file ended mid-structure",
            Self::NotANumber => "NaN is not a valid IconVG number",
            Self::InvalidViewBox => "ViewBox is inverted or infinite",
            Self::InvalidPalette => "palette declares more than 64 entries",
            Self::NonSensibleColor => "colour is not alpha-premultiplied",
            Self::MetadataOrder => "metadata chunks out of order or repeated",
        };
        f.write_str(message)
    }
}

impl core::error::Error for Error {}

/// Decode a graphic's magic identifier and metadata, returning them alongside
/// the offset at which its bytecode begins.
///
/// The bytecode itself is not yet interpreted; see the status note above.
///
/// ```
/// let mut file = iconvg::MAGIC.to_vec();
/// file.push(0x01); // a natural zero: no metadata chunks
///
/// let (metadata, bytecode_at) = iconvg::decode_metadata(&file).unwrap();
///
/// // The documented default when no ViewBox chunk is present.
/// assert_eq!(metadata.view_box.min_x, -32.0);
/// assert_eq!(bytecode_at, file.len());
/// ```
pub fn decode_metadata(bytes: &[u8]) -> Result<(Metadata, usize), Error> {
    meta::decode(bytes)
}
