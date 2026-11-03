// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Geometry ops and whole-file assembly.
//!
//! [`Writer`] is the layer a caller actually uses: it emits path ops, forwards
//! styling to a [`StyleWriter`], and frames the result as a complete graphic —
//! magic identifier, metadata chunks, bytecode.
//!
//! # Repetitions
//!
//! A geometry op carries a run of segments of one kind. The opcode's low four
//! bits hold the count when it is 1 to 15; zero means the count follows as a
//! natural number, biased by 16. [`Writer`] picks whichever spelling is
//! shorter, and splits runs longer than one op can hold.
//!
//! # What is emitted, and what is not
//!
//! The `ViewBox` chunk is written only when it differs from the specification's
//! default of `(-32, -32, +32, +32)`, since an absent chunk means exactly that.
//! The suggested palette is written only when one is supplied. Both rules are
//! pure functions of the input, so the output stays deterministic; they exist
//! because an icon is often a few dozen bytes and a redundant chunk is a
//! meaningful fraction of it.

use super::color::ColorSource;
use super::style::StyleWriter;
use super::{EncodeError, num};
use crate::color::{Palette, Rgba};
use crate::meta::{MAGIC, MID_SUGGESTED_PALETTE, MID_VIEWBOX, ViewBox};
use crate::sink::{GradientKind, Spread};

/// A point in graphic space.
pub type Point = (f32, f32);

/// A quadratic segment: one off-curve control point, then the endpoint.
pub type QuadSegment = (Point, Point);

/// A cubic segment: two off-curve control points, then the endpoint.
pub type CubeSegment = (Point, Point, Point);

/// The largest run of segments one geometry op can hold directly, in its low
/// four bits. Longer runs use the biased natural form.
const INLINE_REPS_MAX: usize = 15;

/// The bias applied to a run length written as a natural number.
const REPS_BIAS: usize = 16;

/// Builds a complete IconVG graphic.
///
/// ```
/// use emblem::encode::Writer;
/// use emblem::ViewBox;
///
/// let mut w = Writer::new(ViewBox { min_x: -24.0, min_y: -24.0, max_x: 24.0, max_y: 24.0 });
/// w.move_to(0.0, -20.0).unwrap();
/// w.parallelogram((2.0, 10.0), (-2.0, 10.0)).unwrap();
/// w.fill_flat(8).unwrap();
/// let bytes = w.finish().unwrap();
/// assert_eq!(&bytes[..4], emblem::MAGIC);
/// ```
#[derive(Clone, Debug)]
pub struct Writer {
    body: Vec<u8>,
    style: StyleWriter,
    view_box: ViewBox,
    suggested_palette: Option<Palette>,
    /// Ops written, which a level-of-detail jump counts rather than bytes.
    ops: usize,
}

impl Writer {
    /// A writer for a graphic with the given clipping rectangle.
    pub fn new(view_box: ViewBox) -> Self {
        Self {
            body: Vec::new(),
            style: StyleWriter::new(),
            view_box,
            suggested_palette: None,
            ops: 0,
        }
    }

    /// Attach a suggested palette, used when a decoder is given none.
    ///
    /// Worth setting even when consumers supply their own: it is what the
    /// graphic looks like on its own terms.
    pub fn suggested_palette(&mut self, palette: Palette) -> &mut Self {
        self.suggested_palette = Some(palette);
        self
    }

    /// The bytecode written so far, without the file framing.
    pub fn bytecode(&self) -> &[u8] {
        &self.body
    }

    /// Where register slot `slot` currently points. See [`StyleWriter`].
    pub fn register_index(&self, slot: u8) -> u8 {
        self.style.register_index(slot)
    }

    /// The current `SEL`.
    pub fn sel(&self) -> u8 {
        self.style.sel()
    }

    // --- geometry -----------------------------------------------------------

    /// Close any open subpath and start a new one at `(x, y)`. Op `0x35`.
    pub fn move_to(&mut self, x: f32, y: f32) -> Result<(), EncodeError> {
        self.ops += 1;
        self.body.push(0x35);
        num::coordinate(x, &mut self.body)?;
        num::coordinate(y, &mut self.body)?;
        Ok(())
    }

    /// Straight segments to each point in turn.
    pub fn line_to(&mut self, points: &[Point]) -> Result<(), EncodeError> {
        self.run(0x00, points.len(), points.iter().flat_map(|p| [p.0, p.1]))
    }

    /// Quadratic segments: one control point and one endpoint each.
    pub fn quad_to(&mut self, points: &[QuadSegment]) -> Result<(), EncodeError> {
        self.run(
            0x10,
            points.len(),
            points.iter().flat_map(|(c, e)| [c.0, c.1, e.0, e.1]),
        )
    }

    /// Cubic segments: two control points and one endpoint each.
    pub fn cube_to(&mut self, points: &[CubeSegment]) -> Result<(), EncodeError> {
        self.run(
            0x20,
            points.len(),
            points
                .iter()
                .flat_map(|(c1, c2, e)| [c1.0, c1.1, c2.0, c2.1, e.0, e.1]),
        )
    }

    /// An ellipse through the pen and the two given points, as `quarters`
    /// cubic arcs. `quarters` runs 1 to 4. Ops `0x30..=0x33`.
    pub fn ellipse(&mut self, quarters: u8, b: Point, c: Point) -> Result<(), EncodeError> {
        if !(1..=4).contains(&quarters) {
            return Err(EncodeError::EllipseQuartersOutOfRange(quarters));
        }
        self.ops += 1;
        self.body.push(0x30 + quarters - 1);
        self.pair(b)?;
        self.pair(c)
    }

    /// A parallelogram with corners at the pen, `b`, `c`, and the implied
    /// fourth. Op `0x34`.
    pub fn parallelogram(&mut self, b: Point, c: Point) -> Result<(), EncodeError> {
        self.ops += 1;
        self.body.push(0x34);
        self.pair(b)?;
        self.pair(c)
    }

    fn pair(&mut self, point: Point) -> Result<(), EncodeError> {
        num::coordinate(point.0, &mut self.body)?;
        num::coordinate(point.1, &mut self.body)
    }

    /// Emit one or more ops of `base` covering `count` repetitions, with the
    /// coordinates from `values`.
    ///
    /// A run longer than one op can express is split. The split is on op
    /// boundaries, so the coordinate stream is untouched.
    fn run(
        &mut self,
        base: u8,
        count: usize,
        values: impl Iterator<Item = f32>,
    ) -> Result<(), EncodeError> {
        if count == 0 {
            return Err(EncodeError::EmptyRun);
        }
        let per_rep = match base {
            0x00 => 2,
            0x10 => 4,
            _ => 6,
        };
        let coords: Vec<f32> = values.collect();
        debug_assert_eq!(coords.len(), count * per_rep);

        // The longest run one op can carry: the biased natural form tops out
        // at the natural maximum plus the bias.
        let max_per_op = num::NATURAL_MAX as usize + REPS_BIAS;
        let mut done = 0;
        while done < count {
            let take = (count - done).min(max_per_op);
            self.ops += 1;
            if take <= INLINE_REPS_MAX {
                self.body.push(base | (take as u8));
            } else {
                self.body.push(base);
                num::natural((take - REPS_BIAS) as u32, &mut self.body)?;
            }
            for value in &coords[done * per_rep..(done + take) * per_rep] {
                num::coordinate(*value, &mut self.body)?;
            }
            done += take;
        }
        Ok(())
    }

    // --- styling, forwarded to the style writer ------------------------------

    /// Move `SEL` by `delta`.
    pub fn adjust_sel(&mut self, delta: u8) {
        self.ops += 1;
        self.style.adjust_sel(delta, &mut self.body);
    }

    /// Put a literal colour and stop position into slot `slot`.
    pub fn set_color(&mut self, slot: u8, color: Rgba, stop: f32) -> Result<(), EncodeError> {
        self.ops += 1;
        self.style.set_color(slot, color, stop, &mut self.body)
    }

    /// Put a blend of two colour references into slot `slot`.
    pub fn set_blend(
        &mut self,
        slot: u8,
        zero: ColorSource,
        one: ColorSource,
        blend: u8,
        stop: f32,
    ) -> Result<(), EncodeError> {
        self.ops += 1;
        self.style
            .set_blend(slot, zero, one, blend, stop, &mut self.body)
    }

    /// Write several consecutive registers.
    pub fn set_registers(&mut self, values: &[u64]) -> Result<(), EncodeError> {
        self.ops += 1;
        self.style.set_registers(values, &mut self.body)
    }

    /// Fill the accumulated path from slot `slot`.
    pub fn fill_flat(&mut self, slot: u8) -> Result<(), EncodeError> {
        self.ops += 1;
        self.style.fill_flat(slot, &mut self.body)
    }

    /// Fill the accumulated path with a gradient.
    pub fn fill_gradient(
        &mut self,
        slot: u8,
        kind: GradientKind,
        spread: Spread,
        stop_count: usize,
        nominal: &[f32],
    ) -> Result<(), EncodeError> {
        self.ops += 1;
        self.style
            .fill_gradient(slot, kind, spread, stop_count, nominal, &mut self.body)
    }

    // --- level of detail ----------------------------------------------------

    /// Emit a branch whose body is drawn only when the rasterization height
    /// falls in `[lod0, lod1)`.
    ///
    /// This is how one file carries a bold simple glyph for small sizes and a
    /// detailed figure for large ones, chosen by the VM rather than by the
    /// caller. Two calls with abutting ranges make the usual two-arm icon:
    ///
    /// ```
    /// use emblem::encode::Writer;
    /// use emblem::ViewBox;
    ///
    /// let mut w = Writer::new(ViewBox::default());
    /// w.level_of_detail(0.0, 32.0, |arm| {
    ///     arm.move_to(0.0, 0.0)?;
    ///     arm.line_to(&[(8.0, 8.0)])?;
    ///     arm.fill_flat(8)
    /// }).unwrap();
    /// w.level_of_detail(32.0, f32::INFINITY, |arm| {
    ///     arm.move_to(0.0, 0.0)?;
    ///     arm.line_to(&[(1.0, 1.0), (2.0, 4.0), (8.0, 8.0)])?;
    ///     arm.fill_flat(8)
    /// }).unwrap();
    /// ```
    ///
    /// # `SEL` must come back where it started
    ///
    /// The op counts a *jump over ops*, so the body is built first and
    /// measured, then the jump written with that count. The subtle part is
    /// `SEL`: an arm that is skipped never runs its register ops, so if an arm
    /// moved `SEL` the value after the branch would depend on which arm the
    /// host chose, and every later slot would address a different register in
    /// different renderings. An arm that does not restore `SEL` is therefore
    /// refused rather than silently emitted.
    pub fn level_of_detail<F>(&mut self, lod0: f32, lod1: f32, build: F) -> Result<(), EncodeError>
    where
        F: FnOnce(&mut Writer) -> Result<(), EncodeError>,
    {
        if lod0.is_nan() || lod1.is_nan() {
            return Err(EncodeError::NotANumber);
        }
        // NaN is already excluded above, so a plain comparison is exact here.
        if lod0 > lod1 {
            return Err(EncodeError::LodRangeInverted { lod0, lod1 });
        }

        // Build the arm against a writer that starts where this one is, so the
        // arm's slots mean what the caller expects.
        let mut arm = Writer::new(self.view_box);
        arm.style = self.style;
        build(&mut arm)?;

        if arm.style.sel() != self.style.sel() {
            return Err(EncodeError::LodArmChangesSel {
                entry: self.style.sel(),
                exit: arm.style.sel(),
            });
        }
        if arm.ops == 0 {
            return Err(EncodeError::EmptyRun);
        }

        // The jump is taken -- skipping the arm -- unless the height falls in
        // the range, so the count is exactly the arm's op count.
        self.ops += 1;
        self.body.push(0x3A);
        num::natural(arm.ops as u32, &mut self.body)?;
        num::coordinate(lod0, &mut self.body)?;
        num::coordinate(lod1, &mut self.body)?;

        self.body.extend_from_slice(&arm.body);
        self.ops += arm.ops;
        Ok(())
    }

    /// How many ops have been written. A level-of-detail jump counts ops, not
    /// bytes, which is why this is tracked.
    pub fn op_count(&self) -> usize {
        self.ops
    }

    // --- assembly -----------------------------------------------------------

    /// Frame the bytecode as a complete graphic.
    pub fn finish(&self) -> Result<Vec<u8>, EncodeError> {
        let mut chunks: Vec<Vec<u8>> = Vec::new();

        // Chunks go in increasing MID order, which the decoder enforces.
        if self.view_box != ViewBox::default() {
            let mut chunk = Vec::new();
            num::natural(MID_VIEWBOX, &mut chunk)?;
            for value in [
                self.view_box.min_x,
                self.view_box.min_y,
                self.view_box.max_x,
                self.view_box.max_y,
            ] {
                num::coordinate(value, &mut chunk)?;
            }
            chunks.push(chunk);
        }

        if let Some(palette) = &self.suggested_palette {
            let mut chunk = Vec::new();
            num::natural(MID_SUGGESTED_PALETTE, &mut chunk)?;
            // The count byte is "entries - 1", so all 64 fit in 0..=63.
            chunk.push((palette.0.len() - 1) as u8);
            for color in &palette.0 {
                super::color::rgba(*color, &mut chunk);
            }
            chunks.push(chunk);
        }

        let mut out = MAGIC.to_vec();
        num::natural(chunks.len() as u32, &mut out)?;
        for chunk in &chunks {
            num::natural(chunk.len() as u32, &mut out)?;
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&self.body);
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
