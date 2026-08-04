//! The magic identifier and the metadata chunks that precede the bytecode.

use crate::color::{Palette, Rgba};
use crate::num;
use crate::Error;

/// Every IconVG graphic starts with `"\x8aIVG"`.
pub const MAGIC: [u8; 4] = [0x8A, 0x49, 0x56, 0x47];

/// MID 8: the clipping rectangle in vector space.
pub const MID_VIEWBOX: u32 = 8;
/// MID 16: default values for the custom palette.
pub const MID_SUGGESTED_PALETTE: u32 = 16;

/// The graphic's clipping rectangle.
///
/// Path nodes may fall outside it, but only pixels within it are painted. Its
/// aspect ratio is a hint for the rasterized pixels, not a mandate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBox {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Default for ViewBox {
    /// `(-32, -32, +32, +32)`, the default when MID 8 is absent.
    fn default() -> Self {
        Self {
            min_x: -32.0,
            min_y: -32.0,
            max_x: 32.0,
            max_y: 32.0,
        }
    }
}

impl ViewBox {
    /// A zero-width or zero-height box is valid: an empty graphic, the way an
    /// empty string is a valid string. Only inverted or infinite is invalid.
    fn validate(self) -> Result<Self, Error> {
        let finite = self.min_x.is_finite()
            && self.min_y.is_finite()
            && self.max_x.is_finite()
            && self.max_y.is_finite();
        if !finite || self.min_x > self.max_x || self.min_y > self.max_y {
            return Err(Error::InvalidViewBox);
        }
        Ok(self)
    }
}

/// What the metadata section carries.
#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub view_box: ViewBox,
    /// The graphic's suggested palette, used as the custom palette when the
    /// caller supplies none.
    pub suggested_palette: Palette,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            view_box: ViewBox::default(),
            suggested_palette: Palette::default(),
        }
    }
}

/// Decode the magic identifier and metadata, returning them with the offset at
/// which the bytecode begins.
pub fn decode(bytes: &[u8]) -> Result<(Metadata, usize), Error> {
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(Error::BadMagic);
    }
    let mut at = MAGIC.len();

    let (chunk_count, used) = num::natural(&bytes[at..])?;
    at += used;

    let mut metadata = Metadata::default();
    let mut previous_mid: Option<u32> = None;

    for _ in 0..chunk_count {
        let (chunk_len, used) = num::natural(&bytes[at..])?;
        at += used;
        let chunk_len = chunk_len as usize;
        let chunk = bytes.get(at..at + chunk_len).ok_or(Error::Truncated)?;
        at += chunk_len;

        let (mid, used) = num::natural(chunk)?;
        let msd = &chunk[used..];

        // Chunks must be in increasing MID order, and MIDs cannot repeat.
        if previous_mid.is_some_and(|previous| mid <= previous) {
            return Err(Error::MetadataOrder);
        }
        previous_mid = Some(mid);

        match mid {
            MID_VIEWBOX => metadata.view_box = decode_view_box(msd)?,
            MID_SUGGESTED_PALETTE => metadata.suggested_palette = decode_palette(msd)?,
            // All MIDs are optional and unknown ones are simply not ours to
            // interpret; skipping beats refusing a file we can still draw.
            _ => {},
        }
    }

    Ok((metadata, at))
}

fn decode_view_box(msd: &[u8]) -> Result<ViewBox, Error> {
    let mut at = 0;
    let mut next = || -> Result<f32, Error> {
        let (value, used) = num::coordinate(&msd[at..])?;
        at += used;
        Ok(value)
    };
    // Bind in order; the closure borrows `at` across calls.
    let min_x = next()?;
    let min_y = next()?;
    let max_x = next()?;
    let max_y = next()?;
    ViewBox {
        min_x,
        min_y,
        max_x,
        max_y,
    }
    .validate()
}

fn decode_palette(msd: &[u8]) -> Result<Palette, Error> {
    let count = *msd.first().ok_or(Error::Truncated)?;
    if count > 63 {
        return Err(Error::InvalidPalette);
    }
    let entries = usize::from(count) + 1;
    let body = msd.get(1..1 + entries * 4).ok_or(Error::Truncated)?;

    // The entries the file does not specify stay opaque black.
    let mut colors = Palette::default().0;
    for (index, rgba) in body.chunks_exact(4).enumerate() {
        let color = Rgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
        if !color.is_sensible() {
            return Err(Error::NonSensibleColor);
        }
        colors[index] = color;
    }
    Ok(Palette(colors))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A natural number in its 1-byte encoding.
    fn nat(value: u8) -> u8 {
        (value << 1) | 1
    }

    /// A coordinate in its 1-byte encoding.
    fn coord(value: i8) -> u8 {
        (((i16::from(value) + 64) as u8) << 1) | 1
    }

    fn graphic(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(nat(chunks.len() as u8));
        for chunk in chunks {
            out.push(nat(chunk.len() as u8));
            out.extend_from_slice(chunk);
        }
        out
    }

    #[test]
    fn a_bare_magic_yields_the_defaults() {
        let bytes = graphic(&[]);
        let (meta, at) = decode(&bytes).unwrap();
        assert_eq!(meta.view_box, ViewBox::default());
        assert_eq!(meta.view_box.min_x, -32.0);
        assert_eq!(meta.suggested_palette, Palette::default());
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn the_wrong_magic_is_refused() {
        assert!(matches!(decode(b"\x89IVG\x01"), Err(Error::BadMagic)));
        assert!(matches!(decode(b"nope"), Err(Error::BadMagic)));
        assert!(matches!(decode(b""), Err(Error::BadMagic)));
    }

    #[test]
    fn a_view_box_chunk_is_read() {
        let chunk = vec![nat(8), coord(-20), coord(-10), coord(20), coord(10)];
        let (meta, _) = decode(&graphic(&[chunk])).unwrap();
        assert_eq!(
            meta.view_box,
            ViewBox {
                min_x: -20.0,
                min_y: -10.0,
                max_x: 20.0,
                max_y: 10.0
            }
        );
    }

    #[test]
    fn a_degenerate_view_box_is_valid_but_an_inverted_one_is_not() {
        let flat = vec![nat(8), coord(5), coord(5), coord(5), coord(5)];
        assert!(decode(&graphic(&[flat])).is_ok(), "zero area is a valid empty graphic");

        let inverted = vec![nat(8), coord(10), coord(0), coord(-10), coord(0)];
        assert!(matches!(
            decode(&graphic(&[inverted])),
            Err(Error::InvalidViewBox)
        ));
    }

    #[test]
    fn a_suggested_palette_fills_its_entries_and_blacks_the_rest() {
        // PalCount 1 means two colours follow.
        let chunk = vec![
            nat(16),
            1,
            0x00, 0xC0, 0x00, 0xC0, // 75%-opaque green
            0xFF, 0x00, 0x00, 0xFF, // opaque red
        ];
        let (meta, _) = decode(&graphic(&[chunk])).unwrap();
        assert_eq!(meta.suggested_palette.0[0], Rgba::new(0x00, 0xC0, 0x00, 0xC0));
        assert_eq!(meta.suggested_palette.0[1], Rgba::new(0xFF, 0x00, 0x00, 0xFF));
        assert_eq!(
            meta.suggested_palette.0[2],
            Rgba::new(0x00, 0x00, 0x00, 0xFF),
            "unspecified entries stay opaque black"
        );
    }

    #[test]
    fn a_non_sensible_palette_colour_is_refused() {
        let chunk = vec![nat(16), 0, 0xFF, 0x00, 0x00, 0x00];
        assert!(matches!(
            decode(&graphic(&[chunk])),
            Err(Error::NonSensibleColor)
        ));
    }

    #[test]
    fn a_palette_count_above_sixty_three_is_refused() {
        let chunk = vec![nat(16), 64];
        assert!(matches!(
            decode(&graphic(&[chunk])),
            Err(Error::InvalidPalette)
        ));
    }

    #[test]
    fn chunks_must_be_in_increasing_mid_order_without_repeats() {
        let view_box = || vec![nat(8), coord(-1), coord(-1), coord(1), coord(1)];
        let palette = || vec![nat(16), 0, 0x00, 0x00, 0x00, 0xFF];

        assert!(decode(&graphic(&[view_box(), palette()])).is_ok());
        assert!(matches!(
            decode(&graphic(&[palette(), view_box()])),
            Err(Error::MetadataOrder)
        ));
        assert!(matches!(
            decode(&graphic(&[view_box(), view_box()])),
            Err(Error::MetadataOrder)
        ));
    }

    #[test]
    fn an_unknown_mid_is_skipped_rather_than_refused() {
        // MID 9 is not one we interpret; the graphic is still readable.
        let unknown = vec![nat(9), 0xDE, 0xAD];
        let (meta, _) = decode(&graphic(&[unknown])).unwrap();
        assert_eq!(meta.view_box, ViewBox::default());
    }

    #[test]
    fn the_bytecode_offset_points_past_the_metadata() {
        let chunk = vec![nat(8), coord(-1), coord(-1), coord(1), coord(1)];
        let mut bytes = graphic(&[chunk]);
        let metadata_len = bytes.len();
        bytes.extend_from_slice(&[0xFF, 0xFF]); // pretend bytecode
        let (_, at) = decode(&bytes).unwrap();
        assert_eq!(at, metadata_len);
    }

    #[test]
    fn a_truncated_chunk_is_an_error_not_a_panic() {
        let mut bytes = MAGIC.to_vec();
        bytes.push(nat(1));
        bytes.push(nat(40)); // claims 40 bytes that are not there
        assert!(matches!(decode(&bytes), Err(Error::Truncated)));
    }
}
