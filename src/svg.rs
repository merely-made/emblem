// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! SVG text from a decoded graphic (feature `svg`).
//!
//! [`SvgSink`] is a [`Sink`] like [`crate::Recorder`]: a lossless projection
//! of the segment and paint sequence. It never adds, merges, reorders or drops
//! a fill the bytes describe: each fill becomes one `<path>`, in call order,
//! with `fill-rule="nonzero"` written out. [`to_svg`] decodes a whole file
//! into it.
//!
//! The output is deterministic. The same bytes, palette, host and options give
//! the same text on every machine, so SVG can be content-addressed like the
//! `.ivg` it came from. Numbers are fixed-point (four fractional digits for
//! coordinates, offsets and opacities; six for gradient geometry), ids are a
//! counter in fill order, and element and attribute order is fixed.
//!
//! The level-of-detail arm is chosen when the text is made, by
//! [`Host::height`]: one text per target height.
//!
//! Where SVG differs from IconVG, and what this does about it:
//!
//! - **Premultiplied interpolation.** IconVG interpolates gradient stops
//!   premultiplied; SVG interpolates straight. Spans between stops of equal
//!   alpha are identical either way. A span whose stops differ in alpha gets
//!   eight intermediate stops computed premultiplied, which approximates the
//!   IconVG curve without matching it exactly.
//! - **`Spread::None`.** IconVG's transparent-outside-`[0, 1]` has no SVG
//!   spread method. It is emulated with a `clipPath` (a strip for linear, a
//!   disc for radial) and `spreadMethod="pad"`; the clip edge may show a faint
//!   antialiased seam.
//! - **Quantized alpha.** Colours arrive as 8-bit premultiplied values, so a
//!   low-alpha colour's straight channels are approximate.
//!
//! Only `svg`, `defs`, `path`, `linearGradient`, `radialGradient`, `stop`,
//! `clipPath`, `rect` and `circle` are emitted. Every value is a formatted
//! number, a `#rrggbb`, a fixed keyword or an id built from a validated
//! [`IdPrefix`]; IconVG metadata carries no text, so nothing else reaches the
//! output.

mod fmt;
mod paint;

use crate::{Host, Paint, Palette, Sink, ViewBox};

/// What can go wrong making SVG text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgError {
    /// The file could not be decoded.
    Decode(crate::Error),
    /// A coordinate, offset or gradient value was `NaN` or infinite.
    NonFinite,
    /// A value too large to write as fixed-point text (about `1.7e34` for a
    /// coordinate).
    OutOfRange,
    /// A gradient matrix with no direction (linear) or no inverse (radial),
    /// which has no SVG equivalent.
    DegenerateGradient,
    /// An id prefix outside `[A-Za-z_][A-Za-z0-9_-]{0,31}`.
    BadPrefix,
}

impl core::fmt::Display for SvgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(error) => write!(f, "cannot decode the graphic: {error}"),
            Self::NonFinite => f.write_str("a value to write is not finite"),
            Self::OutOfRange => f.write_str("a value is too large for fixed-point text"),
            Self::DegenerateGradient => f.write_str("gradient matrix has no SVG equivalent"),
            Self::BadPrefix => f.write_str("id prefix must match [A-Za-z_][A-Za-z0-9_-]{0,31}"),
        }
    }
}

impl core::error::Error for SvgError {}

impl From<crate::Error> for SvgError {
    fn from(error: crate::Error) -> Self {
        Self::Decode(error)
    }
}

/// A prefix for the ids this module writes, so several graphics can share one
/// HTML document without their gradient ids colliding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdPrefix(String);

impl IdPrefix {
    /// Accept `prefix` when it is 1 to 32 characters of `[A-Za-z0-9_-]`
    /// starting with a letter or `_`, so every id it starts is a valid XML id.
    ///
    /// # Errors
    ///
    /// [`SvgError::BadPrefix`] for anything else.
    pub fn new(prefix: &str) -> Result<Self, SvgError> {
        let bytes = prefix.as_bytes();
        let starts_well = bytes
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_');
        let rest_ok = bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-');
        if starts_well && rest_ok && bytes.len() <= 32 {
            Ok(Self(prefix.to_owned()))
        } else {
            Err(SvgError::BadPrefix)
        }
    }

    /// The prefix text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for IdPrefix {
    fn default() -> Self {
        Self("emblem-".to_owned())
    }
}

/// How to write the text.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SvgOptions {
    /// Starts every gradient and clip id: `{prefix}g0`, `{prefix}c0`, ….
    pub id_prefix: IdPrefix,
}

/// Decode `file` and write it as SVG text.
///
/// `host.height` chooses the level-of-detail arm.
///
/// # Errors
///
/// [`SvgError::Decode`] when the file is invalid, and the other variants when
/// a decoded value cannot be written (see [`SvgError`]).
pub fn to_svg(
    file: &[u8],
    palette: &Palette,
    host: Host,
    options: &SvgOptions,
) -> Result<String, SvgError> {
    let (metadata, start) = crate::decode_metadata(file)?;
    let mut sink = SvgSink::new(metadata.view_box, options);
    crate::execute(file, start, palette, host, &mut sink)?;
    sink.finish()
}

/// A [`Sink`] that writes SVG text.
#[derive(Clone, Debug)]
pub struct SvgSink {
    view_box: ViewBox,
    prefix: IdPrefix,
    defs: String,
    body: String,
    path: String,
    /// Every point the current path names, control points included, which
    /// bounds the path for a `Spread::None` clip strip.
    points: Vec<(f32, f32)>,
    gradients: u32,
    clips: u32,
    error: Option<SvgError>,
}

impl SvgSink {
    /// A sink for a graphic with this ViewBox.
    pub fn new(view_box: ViewBox, options: &SvgOptions) -> Self {
        Self {
            view_box,
            prefix: options.id_prefix.clone(),
            defs: String::new(),
            body: String::new(),
            path: String::new(),
            points: Vec::new(),
            gradients: 0,
            clips: 0,
            error: None,
        }
    }

    /// The finished document.
    ///
    /// # Errors
    ///
    /// The first value that could not be written, if any.
    pub fn finish(self) -> Result<String, SvgError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let ViewBox {
            min_x,
            min_y,
            max_x,
            max_y,
        } = self.view_box;
        let mut out = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox=""#);
        let (min_x, min_y) = (f64::from(min_x), f64::from(min_y));
        let box_values = [
            min_x,
            min_y,
            f64::from(max_x) - min_x,
            f64::from(max_y) - min_y,
        ];
        for (i, value) in box_values.into_iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            fmt::number(&mut out, value, fmt::COORD)?;
        }
        out.push_str("\">\n");
        if !self.defs.is_empty() {
            out.push_str("<defs>\n");
            out.push_str(&self.defs);
            out.push_str("</defs>\n");
        }
        out.push_str(&self.body);
        out.push_str("</svg>\n");
        Ok(out)
    }

    fn segment(&mut self, letter: char, coords: &[f32]) {
        if !self.path.is_empty() {
            self.path.push(' ');
        }
        self.path.push(letter);
        for (i, pair) in coords.as_chunks::<2>().0.iter().enumerate() {
            self.points.push((pair[0], pair[1]));
            for (j, value) in pair.iter().enumerate() {
                if i + j > 0 {
                    self.path.push(' ');
                }
                let result = fmt::number(&mut self.path, f64::from(*value), fmt::COORD);
                self.note(result);
            }
        }
    }

    fn note(&mut self, result: Result<(), SvgError>) {
        if let (Err(error), None) = (result, self.error) {
            self.error = Some(error);
        }
    }

    fn write_fill(&mut self, paint: &Paint) -> Result<(), SvgError> {
        let mut attrs = String::new();
        match paint {
            Paint::Flat(color) => {
                attrs.push_str(" fill=\"");
                let (rgb, alpha) = paint::straight(*color);
                fmt::hex(&mut attrs, rgb);
                attrs.push('"');
                opacity(&mut attrs, " fill-opacity", alpha)?;
                attrs.push_str(r#" fill-rule="nonzero""#);
            },
            Paint::Gradient {
                kind,
                spread,
                stops,
                matrix,
            } => {
                let id = format!("{}g{}", self.prefix.as_str(), self.gradients);
                let clip = format!("{}c{}", self.prefix.as_str(), self.clips);
                let gradient = paint::Gradient {
                    kind: *kind,
                    spread: *spread,
                    stops,
                    matrix: *matrix,
                };
                let clipped = paint::write(&mut self.defs, &id, &clip, &gradient, &self.points)?;
                self.gradients += 1;
                attrs.push_str(&format!(r#" fill="url(#{id})" fill-rule="nonzero""#));
                if clipped {
                    self.clips += 1;
                    attrs.push_str(&format!(r#" clip-path="url(#{clip})""#));
                }
            },
        }
        self.body.push_str("<path d=\"");
        self.body.push_str(&self.path);
        self.body.push('"');
        self.body.push_str(&attrs);
        self.body.push_str("/>\n");
        Ok(())
    }
}

/// Write `name="alpha/255"` unless the colour is opaque.
fn opacity(out: &mut String, name: &str, alpha: u8) -> Result<(), SvgError> {
    if alpha == u8::MAX {
        return Ok(());
    }
    out.push_str(name);
    out.push_str("=\"");
    fmt::number(out, f64::from(alpha) / 255.0, fmt::COORD)?;
    out.push('"');
    Ok(())
}

impl Sink for SvgSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.segment('M', &[x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.segment('L', &[x, y]);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.segment('Q', &[cx, cy, x, y]);
    }
    fn cube_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.segment('C', &[c1x, c1y, c2x, c2y, x, y]);
    }
    fn close(&mut self) {
        self.segment('Z', &[]);
    }
    fn fill(&mut self, paint: &Paint) {
        if self.error.is_none() {
            let result = self.write_fill(paint);
            self.note(result);
        }
        self.path.clear();
        self.points.clear();
    }
}
