//! # iconvg
//!
//! A pure-Rust implementation of [IconVG](https://github.com/nigeltao/iconvg),
//! a compact binary format for simple vector graphics: icons, logos, glyphs
//! and emoji. It is far simpler than SVG on purpose, with no text, multimedia,
//! interactivity, scripting, animation or DOM, and it is a presentation format
//! rather than an authoring one: strokes are already flattened to filled paths
//! before they reach a file.
//!
//! ## Status: complete decoder, and the format is experimental
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
//!   suggested palette;
//! - [`vm`] — the bytecode virtual machine: every geometry, register, fill,
//!   control-flow and reserved op, including jumps, calls, feature detection
//!   and level-of-detail branching.
//!
//! ## It decodes; it does not rasterize
//!
//! [`execute`] drives a [`Sink`] with path segments and fills. That is the
//! whole output: IconVG is a presentation format whose strokes are already
//! flattened to filled paths, so what a consumer needs is the path stream,
//! which every 2D backend already takes. Rasterizing here would mean
//! reimplementing — worse — what vello, tiny-skia or a canvas already do.
//!
//! One thing a backend must get right: IconVG fills with the **non-zero
//! winding** rule, never even-odd. A backend defaulting to even-odd renders
//! self-intersecting icons wrongly and silently.
//!
//! ```
//! # fn main() -> Result<(), iconvg::Error> {
//! // Material Design's "action/info" icon, the specification's own example.
//! let file: [u8; 36] = [
//!     0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35,
//!     0x81, 0x59, 0x33, 0x59, 0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d,
//!     0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d, 0x75, 0x7d, 0x6d, 0x88,
//! ];
//!
//! let (metadata, bytecode_at) = iconvg::decode_metadata(&file)?;
//! let mut sink = iconvg::Recorder::default();
//! iconvg::execute(
//!     &file,
//!     bytecode_at,
//!     &iconvg::Palette::default(),
//!     iconvg::Host::default(),
//!     &mut sink,
//! )?;
//!
//! assert_eq!(metadata.view_box.min_x, -24.0);
//! assert_eq!(sink.fill_count(), 1);
//! # Ok(())
//! # }
//! ```
//!
//! ## Why a graphic always terminates
//!
//! Worth knowing, because it is why there is no fuel counter or recursion
//! limit to tune. Jump ops "propel the PC forward (never backward)", so
//! bytecode cannot loop; and "IconVG bytecode calls cannot nest — the maximum
//! call stack depth is one", so a Call while one is outstanding is invalid.
//! Between them the format itself bounds execution.
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
mod oplen;
pub mod sink;
pub mod vm;

pub use color::{Palette, Registers, Rgba, built_in_palette};
pub use meta::{MAGIC, Metadata, ViewBox};
pub use sink::{Call, GradientKind, Matrix, Paint, Recorder, Sink, Spread, Stop};
pub use vm::{Host, execute};

/// The result of reading an IconVG graphic.
pub type Result<T> = core::result::Result<T, Error>;

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
    /// A Call op ran while another was outstanding. IconVG calls cannot nest:
    /// the maximum stack depth is one, and that is what bounds recursion.
    NestedCall,
    /// A Call Transformed supplied a matrix with no inverse. Gradients are
    /// defined against that inverse, so the graphic has no meaning.
    SingularMatrix,
    /// A Call named a Segment Type other than `0x00`. Every other value is
    /// reserved, and the specification gives no fallback.
    ReservedSegmentType,
    /// A SegRef pointed outside the file, or an Absolute Direct one had a
    /// zero offset.
    BadSegRef,
    /// A gradient declared 63 in its stop-count field, which is invalid.
    BadGradient,
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
            Self::NestedCall => "IconVG calls cannot nest; the maximum depth is one",
            Self::SingularMatrix => "transformation matrix has no inverse",
            Self::ReservedSegmentType => "segment type is reserved and cannot be called",
            Self::BadSegRef => "segment reference points outside the file",
            Self::BadGradient => "gradient stop count of 63 is invalid",
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
pub fn decode_metadata(bytes: &[u8]) -> Result<(Metadata, usize)> {
    meta::decode(bytes)
}
