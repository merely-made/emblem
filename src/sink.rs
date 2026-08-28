// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Where a decoded graphic goes: path segments and fills.
//!
//! The VM does not rasterize. IconVG is a presentation format whose strokes
//! are already flattened to filled paths, so what a consumer actually needs is
//! the sequence of path segments and the paint to fill them with -- which is
//! exactly what every 2D backend already takes. Rasterizing here would mean
//! reimplementing, worse, what vello, tiny-skia or a browser's canvas already
//! do.
//!
//! So [`Sink`] is the seam. Implement it over whatever path builder is at
//! hand; the VM calls it in the order the bytecode dictates.

use crate::color::Rgba;

/// How a gradient extrapolates outside the `[0, 1]` stop range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spread {
    /// Below `0` and above `1` map to transparent black.
    None,
    /// Below `0` and above `1` map to what `0` and `1` map to.
    Pad,
    /// The mapping reflects, start-to-end then end-to-start, repeatedly.
    Reflect,
    /// The mapping repeats start-to-end.
    Repeat,
}

impl Spread {
    pub(crate) fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 => Self::None,
            0x01 => Self::Pad,
            0x02 => Self::Reflect,
            _ => Self::Repeat,
        }
    }
}

/// One gradient stop: where it sits, and what colour it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// Position along the gradient. Decoded from a register's low 32 bits as
    /// an unsigned 16.16 fixed point value.
    pub offset: f32,
    /// The resolved, alpha-premultiplied colour.
    pub color: Rgba,
}

/// An affine 3-by-2 matrix `[a, b, c; d, e, f]`, mapping
/// `(x, y)` to `(a*x + b*y + c, d*x + e*y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f32; 6]);

impl Matrix {
    /// The identity.
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    /// Apply this matrix to a point.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + b * y + c, d * x + e * y + f)
    }

    /// The inverse, or `None` when this matrix is singular.
    ///
    /// The VM needs this because the Global Backwards Transformation Matrix is
    /// the inverse of the forward one, and gradients are defined against it.
    pub fn inverse(&self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * e - b * d;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        Some(Self([
            e / det,
            -b / det,
            (b * f - c * e) / det,
            -d / det,
            a / det,
            (c * d - a * f) / det,
        ]))
    }
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// What to fill a path with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A single uniform colour.
    Flat(Rgba),
    /// A gradient. Linear ranges from `x = 0` to `x = 1` in gradient
    /// coordinate space; radial has centre `(0, 0)` and radius `1`. The
    /// [`Paint::matrix`] maps graphic space into that space.
    Gradient {
        /// Whether the gradient is linear or radial.
        kind: GradientKind,
        /// How it extrapolates outside `[0, 1]`.
        spread: Spread,
        /// Between 2 and 64 stops, positions non-decreasing from 0 to 1.
        stops: Vec<Stop>,
        /// The Effective Gradient Matrix: graphic space to gradient space.
        matrix: Matrix,
    },
}

impl Paint {
    /// The gradient matrix, when this is a gradient.
    pub fn matrix(&self) -> Option<Matrix> {
        match self {
            Self::Flat(_) => None,
            Self::Gradient { matrix, .. } => Some(*matrix),
        }
    }
}

/// Whether a gradient is linear or radial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientKind {
    /// Ranges from `x = 0` to `x = 1` in gradient coordinate space.
    Linear,
    /// Centre `(0, 0)`, radius `1`, in gradient coordinate space.
    Radial,
}

/// Receives the path segments and fills a graphic is made of.
///
/// Every coordinate handed to a `Sink` is already transformed by the VM's
/// Global Forward Transformation Matrix, so an implementation places points
/// directly and applies no transform of its own.
pub trait Sink {
    /// Begin a new subpath at `(x, y)`.
    fn move_to(&mut self, x: f32, y: f32);

    /// A straight segment from the pen to `(x, y)`.
    fn line_to(&mut self, x: f32, y: f32);

    /// A quadratic Bézier via `(cx, cy)` to `(x, y)`.
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32);

    /// A cubic Bézier via two control points to `(x, y)`.
    fn cube_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32);

    /// Close the current subpath.
    fn close(&mut self);

    /// Fill everything accumulated since the last fill, then clear it.
    ///
    /// IconVG always uses the **non-zero winding** rule, never even-odd. A
    /// backend defaulting to even-odd will render self-intersecting icons
    /// wrongly, and silently.
    fn fill(&mut self, paint: &Paint);
}

/// A `Sink` that records calls, for tests and for inspecting a graphic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorder {
    /// Every call, in order.
    pub calls: Vec<Call>,
}

/// One recorded [`Sink`] call.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// [`Sink::move_to`].
    MoveTo(f32, f32),
    /// [`Sink::line_to`].
    LineTo(f32, f32),
    /// [`Sink::quad_to`].
    QuadTo(f32, f32, f32, f32),
    /// [`Sink::cube_to`].
    CubeTo(f32, f32, f32, f32, f32, f32),
    /// [`Sink::close`].
    Close,
    /// [`Sink::fill`].
    Fill(Paint),
}

impl Sink for Recorder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.calls.push(Call::MoveTo(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.calls.push(Call::LineTo(x, y));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.calls.push(Call::QuadTo(cx, cy, x, y));
    }
    fn cube_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.calls.push(Call::CubeTo(c1x, c1y, c2x, c2y, x, y));
    }
    fn close(&mut self) {
        self.calls.push(Call::Close);
    }
    fn fill(&mut self, paint: &Paint) {
        self.calls.push(Call::Fill(paint.clone()));
    }
}

impl Recorder {
    /// How many fills this graphic performed.
    pub fn fill_count(&self) -> usize {
        self.calls
            .iter()
            .filter(|call| matches!(call, Call::Fill(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_maps_a_point_to_itself() {
        assert_eq!(Matrix::IDENTITY.apply(3.0, 4.0), (3.0, 4.0));
    }

    #[test]
    fn an_inverse_undoes_its_matrix() {
        // The GBTM is the GFTM's inverse, and gradients are defined against
        // it, so an error here misplaces every gradient rather than failing.
        let forward = Matrix([2.0, 0.5, 10.0, -1.0, 3.0, -4.0]);
        let backward = forward.inverse().expect("not singular");
        let (x, y) = forward.apply(7.0, -2.0);
        let (rx, ry) = backward.apply(x, y);
        assert!((rx - 7.0).abs() < 1e-4, "round tripped x to {rx}");
        assert!((ry + 2.0).abs() < 1e-4, "round tripped y to {ry}");
    }

    #[test]
    fn a_singular_matrix_has_no_inverse() {
        // Two parallel rows: determinant zero. Must not produce infinities.
        let flat = Matrix([1.0, 2.0, 0.0, 2.0, 4.0, 0.0]);
        assert_eq!(flat.inverse(), None);
    }

    #[test]
    fn spread_reads_only_its_two_bits() {
        assert_eq!(Spread::from_bits(0x00), Spread::None);
        assert_eq!(Spread::from_bits(0x01), Spread::Pad);
        assert_eq!(Spread::from_bits(0x02), Spread::Reflect);
        assert_eq!(Spread::from_bits(0x03), Spread::Repeat);
    }
}
