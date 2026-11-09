// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Gradients and colours as SVG paint servers.
//!
//! A [`crate::Paint`] gradient's matrix maps sink coordinates into gradient
//! space, where linear runs from `x = 0` to `x = 1` and radial is the unit
//! disc. SVG wants the opposite direction, so:
//!
//! - **Linear** colour depends only on `t = a*x + b*y + c`, the matrix's first
//!   row. The points where `t` is 0 and 1 nearest the origin of that line are
//!   `p0 = (-a*c, -b*c) / n` and `p1 = p0 + (a, b) / n` with `n = a² + b²`,
//!   which become `x1 y1 x2 y2` in user space and need no transform.
//! - **Radial** is the unit circle with the inverse matrix as its
//!   `gradientTransform`.

use super::{SvgError, fmt};
use crate::color::Rgba;
use crate::sink::{GradientKind, Matrix, Spread, Stop};

/// Intermediate stops written between two stops of different alpha, so SVG's
/// straight interpolation follows IconVG's premultiplied one closely.
const ALPHA_STEPS: u32 = 8;

pub(super) struct Gradient<'a> {
    pub kind: GradientKind,
    pub spread: Spread,
    pub stops: &'a [Stop],
    pub matrix: Matrix,
}

/// Straight `rgb` and alpha from a premultiplied colour, rounding as the vello
/// bridge does. Alpha 0 is black.
pub(super) fn straight(color: Rgba) -> ([u8; 3], u8) {
    if color.a == 0 {
        return ([0; 3], 0);
    }
    let alpha = u32::from(color.a);
    let channel = |c: u8| ((u32::from(c) * 255 + alpha / 2) / alpha).min(255) as u8;
    (
        [channel(color.r), channel(color.g), channel(color.b)],
        color.a,
    )
}

/// Write the gradient (and, for `Spread::None`, its clip) into `defs`.
/// Returns whether the path needs `clip-path`.
pub(super) fn write(
    defs: &mut String,
    id: &str,
    clip: &str,
    gradient: &Gradient<'_>,
    points: &[(f32, f32)],
) -> Result<bool, SvgError> {
    let clipped = gradient.spread == Spread::None;
    let mut out = String::new();
    let close = match gradient.kind {
        GradientKind::Linear => {
            let line = Line::from(gradient.matrix)?;
            out.push_str(&format!(
                r#"<linearGradient id="{id}" gradientUnits="userSpaceOnUse""#
            ));
            let ends = [line.origin, line.end()];
            for (name, value) in ["x1", "y1", "x2", "y2"].iter().zip(ends.as_flattened()) {
                attr(&mut out, name, *value)?;
            }
            if clipped {
                line.clip(defs, clip, points)?;
            }
            "</linearGradient>\n"
        },
        GradientKind::Radial => {
            let inverse = inverse(gradient.matrix)?;
            out.push_str(&format!(
                r#"<radialGradient id="{id}" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="1""#
            ));
            out.push_str(" gradientTransform=\"");
            transform(&mut out, &inverse)?;
            out.push('"');
            if clipped {
                defs.push_str(&format!(
                    r#"<clipPath id="{clip}" clipPathUnits="userSpaceOnUse"><circle transform=""#
                ));
                transform(defs, &inverse)?;
                defs.push_str("\" cx=\"0\" cy=\"0\" r=\"1\"/></clipPath>\n");
            }
            "</radialGradient>\n"
        },
    };
    let method = match gradient.spread {
        Spread::None | Spread::Pad => "pad",
        Spread::Reflect => "reflect",
        Spread::Repeat => "repeat",
    };
    out.push_str(&format!(" spreadMethod=\"{method}\">\n"));
    stops(&mut out, gradient.stops)?;
    out.push_str(close);
    defs.push_str(&out);
    Ok(clipped)
}

/// A linear gradient's axis: where `t = 0` and which way `t` grows.
struct Line {
    origin: [f64; 2],
    along: [f64; 2],
    /// The matrix's first row, `t = a*x + b*y + c`.
    row: [f64; 2],
}

impl Line {
    fn from(matrix: Matrix) -> Result<Self, SvgError> {
        let [a, b, c, ..] = matrix.0.map(f64::from);
        let n = a * a + b * b;
        if !n.is_finite() || n == 0.0 || !c.is_finite() {
            return Err(SvgError::DegenerateGradient);
        }
        Ok(Self {
            origin: [-a * c / n, -b * c / n],
            along: [a / n, b / n],
            row: [a, b],
        })
    }

    fn end(&self) -> [f64; 2] {
        [
            self.origin[0] + self.along[0],
            self.origin[1] + self.along[1],
        ]
    }

    /// A clip strip covering `0 <= t <= 1` and the whole path across it. In
    /// the rect's own space `x` is `t`, and `y` runs across the axis, so the
    /// path's points bound `y`.
    fn clip(&self, defs: &mut String, clip: &str, points: &[(f32, f32)]) -> Result<(), SvgError> {
        let [a, b] = self.row;
        let n = a * a + b * b;
        let across = [-b / n, a / n];
        let (mut low, mut high) = (0.0f64, 0.0f64);
        for (i, &(x, y)) in points.iter().enumerate() {
            let (dx, dy) = (f64::from(x) - self.origin[0], f64::from(y) - self.origin[1]);
            let v = -b * dx + a * dy;
            if i == 0 || v < low {
                low = v;
            }
            if i == 0 || v > high {
                high = v;
            }
        }
        defs.push_str(&format!(
            r#"<clipPath id="{clip}" clipPathUnits="userSpaceOnUse"><rect transform=""#
        ));
        let rect_to_user = [
            self.along[0],
            self.along[1],
            across[0],
            across[1],
            self.origin[0],
            self.origin[1],
        ];
        transform(defs, &rect_to_user)?;
        defs.push('"');
        attr(defs, "x", 0.0)?;
        attr(defs, "y", low)?;
        attr(defs, "width", 1.0)?;
        attr(defs, "height", high - low)?;
        defs.push_str("/></clipPath>\n");
        Ok(())
    }
}

/// The inverse of `matrix`, as SVG's `matrix(a b c d e f)` column order: user
/// space from gradient space.
fn inverse(matrix: Matrix) -> Result<[f64; 6], SvgError> {
    let [a, b, c, d, e, f] = matrix
        .inverse()
        .ok_or(SvgError::DegenerateGradient)?
        .0
        .map(f64::from);
    Ok([a, d, b, e, c, f])
}

fn transform(out: &mut String, m: &[f64; 6]) -> Result<(), SvgError> {
    out.push_str("matrix(");
    for (i, value) in m.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        fmt::number(out, *value, fmt::GRADIENT)?;
    }
    out.push(')');
    Ok(())
}

fn attr(out: &mut String, name: &str, value: f64) -> Result<(), SvgError> {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    fmt::number(out, value, fmt::GRADIENT)?;
    out.push('"');
    Ok(())
}

fn stops(out: &mut String, stops: &[Stop]) -> Result<(), SvgError> {
    for (i, stop) in stops.iter().enumerate() {
        let (rgb, alpha) = straight(stop.color);
        write_stop(out, f64::from(stop.offset), rgb, alpha)?;
        let Some(next) = stops.get(i + 1) else { break };
        if next.color.a == stop.color.a {
            continue;
        }
        let (o0, o1) = (f64::from(stop.offset), f64::from(next.offset));
        for k in 1..=ALPHA_STEPS {
            let t = f64::from(k) / f64::from(ALPHA_STEPS + 1);
            let (rgb, alpha) = mix(stop.color, next.color, t);
            write_stop(out, o0 + (o1 - o0) * t, rgb, alpha)?;
        }
    }
    Ok(())
}

/// Premultiplied interpolation, then straight colour.
fn mix(from: Rgba, to: Rgba, t: f64) -> ([u8; 3], u8) {
    let lerp = |p: u8, q: u8| f64::from(p) + (f64::from(q) - f64::from(p)) * t;
    let alpha = lerp(from.a, to.a);
    let round = |v: f64| (v + 0.5).min(255.0) as u8;
    if alpha <= 0.0 {
        return ([0; 3], 0);
    }
    let channel = |p: u8, q: u8| round(lerp(p, q) * 255.0 / alpha);
    (
        [
            channel(from.r, to.r),
            channel(from.g, to.g),
            channel(from.b, to.b),
        ],
        round(alpha),
    )
}

fn write_stop(out: &mut String, offset: f64, rgb: [u8; 3], alpha: u8) -> Result<(), SvgError> {
    out.push_str("<stop offset=\"");
    fmt::number(out, offset, fmt::COORD)?;
    out.push_str("\" stop-color=\"");
    fmt::hex(out, rgb);
    out.push('"');
    super::opacity(out, " stop-opacity", alpha)?;
    out.push_str("/>\n");
    Ok(())
}
