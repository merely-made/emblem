// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use emblem::encode::color::ColorSource;
use emblem::encode::{EncodeError, Point, Writer};
use emblem::{GradientKind, Spread, ViewBox};

use super::{ACTIVITY, BackdropKind, CLEAR, MOTIF, ViewDigest, WASH, digest::ClusterCell};

/// Motifs appear at this raster height and above, in device pixels.
pub const LOD_HEIGHT: f32 = 256.0;
/// The entire file, including metadata.
pub const MAX_BYTES: usize = 4096;
/// All subpaths in the file, including both sides of each contour.
pub const MAX_PATHS: usize = 64;

#[derive(Debug)]
pub enum BackdropError {
    IconVg(EncodeError),
    TooManyPaths,
    TooManyBytes,
}

impl From<EncodeError> for BackdropError {
    fn from(error: EncodeError) -> Self {
        Self::IconVg(error)
    }
}

impl std::fmt::Display for BackdropError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IconVg(error) => write!(f, "{error}"),
            Self::TooManyPaths => write!(f, "wash exceeds {MAX_PATHS} paths"),
            Self::TooManyBytes => write!(f, "wash exceeds {MAX_BYTES} bytes"),
        }
    }
}

impl std::error::Error for BackdropError {}

/// Encode the wash in the default IconVG view box. The view box maps onto the
/// digest's snapped world bounds. The caller supplies the wash palette.
///
/// # Errors
///
/// Returns an error if the encoder refuses a drawing or either output limit
/// is exceeded.
pub fn derive_backdrop(digest: &ViewDigest) -> Result<Vec<u8>, BackdropError> {
    encode(digest, WASH, Budget::new(MAX_PATHS, MAX_BYTES))
}

fn encode(
    digest: &ViewDigest,
    grammar: crate::Grammar,
    mut budget: Budget,
) -> Result<Vec<u8>, BackdropError> {
    let mut w = Writer::new(ViewBox::default());
    // Stamp both versions into two unused register positions. No fill uses
    // them, and subsequent palette references read the host palette directly.
    w.set_registers(&[
        u64::from(grammar.seed_version),
        u64::from(grammar.drawing_version),
    ])?;
    w.adjust_sel(2);

    let bounds = digest.bounds();
    for cluster in &digest.clusters {
        draw_cluster(&mut w, &mut budget, cluster, bounds, digest.activity)?;
    }
    // The common wash and contours are outside the branch and occur once.
    budget.take_paths(MOTIF_PATHS)?;
    w.level_of_detail(LOD_HEIGHT, f32::INFINITY, |arm| {
        draw_motif(arm, digest.dominant_kind(), digest.density)
    })?;
    budget.finish(w.finish()?)
}

fn draw_cluster(
    w: &mut Writer,
    budget: &mut Budget,
    cluster: &ClusterCell,
    bounds: [i32; 4],
    activity: u8,
) -> Result<(), BackdropError> {
    let (x, y) = place(cluster.center, bounds);
    let hull: Vec<_> = cluster
        .hull
        .iter()
        .map(|&cell| place(cell, bounds))
        .collect();
    let step = |axis: usize| 64.0 / (f64::from(bounds[axis + 2]) - f64::from(bounds[axis])) as f32;
    let (mut rx, mut ry) = (step(0), step(1));
    for &(hx, hy) in &hull {
        rx = rx.max((hx - x).abs() + step(0));
        ry = ry.max((hy - y).abs() + step(1));
    }

    let entry = ColorSource::Custom(cluster.entry);
    let clear = ColorSource::Custom(CLEAR);
    w.set_blend(1, entry, ColorSource::Custom(ACTIVITY), activity * 32, 0.0)?;
    w.set_blend(2, entry, clear, 128, 0.5)?;
    w.set_blend(3, clear, clear, 0, 1.0 - 1.0 / 65536.0)?;
    budget.take_paths(1)?;
    rectangle(
        w,
        (x - rx).max(-32.0),
        (y - ry).max(-32.0),
        (x + rx).min(32.0),
        (y + ry).min(32.0),
    )?;
    w.fill_gradient(
        1,
        GradientKind::Radial,
        Spread::Pad,
        3,
        &[rx.recip(), 0.0, -x / rx, 0.0, ry.recip(), -y / ry],
    )?;

    if hull.len() >= 3 && area(&hull) != 0.0 {
        let count = hull.len() as f32;
        let (cx, cy) = hull
            .iter()
            .fold((0.0, 0.0), |(cx, cy), &(x, y)| (cx + x, cy + y));
        let (cx, cy) = (cx / count, cy / count);
        let inner: Vec<_> = hull
            .iter()
            .rev()
            .map(|&(x, y)| (cx + (x - cx) * 0.78, cy + (y - cy) * 0.78))
            .collect();
        budget.take_paths(2)?;
        polygon(w, &hull)?;
        polygon(w, &inner)?;
        w.set_blend(1, entry, clear, 192, 0.0)?;
        w.fill_flat(1)?;
    }
    Ok(())
}

fn place(cell: [i32; 2], bounds: [i32; 4]) -> Point {
    let scale = |axis: usize| {
        let low = f64::from(bounds[axis]);
        let span = f64::from(bounds[axis + 2]) - low;
        ((f64::from(cell[axis]) + 0.5 - low) * 64.0 / span - 32.0) as f32
    };
    (scale(0), scale(1))
}

fn area(points: &[Point]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&(x, y), &(next_x, next_y))| {
            f64::from(x) * f64::from(next_y) - f64::from(y) * f64::from(next_x)
        })
        .sum()
}

fn rectangle(
    w: &mut Writer,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Result<(), EncodeError> {
    w.move_to(left, top)?;
    w.parallelogram((right, top), (right, bottom))
}

fn polygon(w: &mut Writer, points: &[Point]) -> Result<(), EncodeError> {
    w.move_to(points[0].0, points[0].1)?;
    w.line_to(&points[1..])
}

const MOTIF_AXIS: [f32; 3] = [-1.0, 0.0, 1.0];
const MOTIF_PATHS: usize = MOTIF_AXIS.len() * MOTIF_AXIS.len();

fn draw_motif(w: &mut Writer, kind: BackdropKind, density: u8) -> Result<(), EncodeError> {
    let pitch = 24.0 - f32::from(density) * 2.0;
    let shape: &[Point] = match kind {
        BackdropKind::Document => &[(-2., -3.), (2., -3.), (2., 3.), (-2., 3.)],
        BackdropKind::Image => &[(0., -3.), (3., 0.), (0., 3.), (-3., 0.)],
        BackdropKind::Audio => &[(-3., -1.), (3., -1.), (3., 1.), (-3., 1.)],
        BackdropKind::Video => &[(-2., -3.), (3., 0.), (-2., 3.)],
        BackdropKind::Data => &[(-3., -2.), (1., -2.), (3., 2.), (-1., 2.)],
        BackdropKind::Collection => &[
            (-3., 0.),
            (-2., -2.),
            (2., -2.),
            (3., 0.),
            (2., 2.),
            (-2., 2.),
        ],
        BackdropKind::Other => &[(-2., -2.), (2., -2.), (2., 2.), (-2., 2.)],
    };
    for row in MOTIF_AXIS {
        for column in MOTIF_AXIS {
            let points: Vec<_> = shape
                .iter()
                .map(|&(x, y)| (column * pitch + x, row * pitch + y))
                .collect();
            polygon(w, &points)?;
        }
    }
    w.fill_flat(crate::FIRST_PALETTE_SLOT + MOTIF)
}

struct Budget {
    remaining_paths: usize,
    max_bytes: usize,
}

impl Budget {
    const fn new(max_paths: usize, max_bytes: usize) -> Self {
        Self {
            remaining_paths: max_paths,
            max_bytes,
        }
    }

    fn take_paths(&mut self, paths: usize) -> Result<(), BackdropError> {
        self.remaining_paths = self
            .remaining_paths
            .checked_sub(paths)
            .ok_or(BackdropError::TooManyPaths)?;
        Ok(())
    }

    fn finish(self, bytes: Vec<u8>) -> Result<Vec<u8>, BackdropError> {
        if bytes.len() > self.max_bytes {
            return Err(BackdropError::TooManyBytes);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
