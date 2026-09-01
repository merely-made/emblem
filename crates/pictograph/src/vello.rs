// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Decode IconVG into the exact vello [`Scene`] type consumed by netrender.
//!
//! This module is behind pictograph's `vello` feature. The byte generator does
//! not otherwise acquire a rendering stack. Path coordinates arrive from
//! emblem already transformed, so the bridge only builds a [`kurbo::BezPath`],
//! converts the resolved paint, and fills with [`peniko::Fill::NonZero`].

use emblem::{
    GradientKind as EmblemGradientKind, Host, Matrix, Paint, Palette, Rgba, Sink as EmblemSink,
    Spread, Stop, ViewBox,
};

pub use netrender_vello::{Scene, kurbo, peniko};

use kurbo::{Affine, BezPath, Circle, Point, Rect, Shape};
use peniko::{Color, ColorStop, Extend, Fill, Gradient, InterpolationAlphaSpace};

/// A failure while lowering decoded IconVG into a vello scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The IconVG metadata or bytecode was invalid.
    Iconvg(emblem::Error),
    /// A gradient transform could not be represented by vello.
    InvalidGradientMatrix,
}

impl From<emblem::Error> for Error {
    fn from(error: emblem::Error) -> Self {
        Self::Iconvg(error)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Iconvg(error) => write!(f, "IconVG decode failed: {error}"),
            Self::InvalidGradientMatrix => {
                f.write_str("IconVG gradient matrix cannot be represented by vello")
            }
        }
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Iconvg(error) => Some(error),
            Self::InvalidGradientMatrix => None,
        }
    }
}

/// A decoded graphic and the ViewBox that places it.
///
/// The scene carries the ViewBox clip. Append it to a frame scene with the
/// transform that maps graphic coordinates into the destination face.
pub struct Graphic {
    scene: Scene,
    view_box: ViewBox,
}

impl Graphic {
    /// The decoded vello scene fragment.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The IconVG clipping rectangle and placement hint.
    pub fn view_box(&self) -> ViewBox {
        self.view_box
    }

    /// Append this fragment to a caller-owned scene.
    pub fn append_to(&self, target: &mut Scene, transform: Option<Affine>) {
        target.append(&self.scene, transform);
    }

    /// Consume the wrapper and return its scene fragment.
    pub fn into_scene(self) -> Scene {
        self.scene
    }
}

/// An emblem sink backed by a vello scene and a kurbo path builder.
///
/// Call [`VelloSink::into_scene`] after emblem finishes driving the sink. That
/// final step reports a gradient transform that vello cannot represent.
pub struct VelloSink {
    scene: Scene,
    path: BezPath,
    error: Option<Error>,
}

impl VelloSink {
    /// Start an empty scene fragment.
    pub fn new() -> Self {
        Self {
            scene: Scene::new(),
            path: BezPath::new(),
            error: None,
        }
    }

    /// Finish lowering and return the scene.
    pub fn into_scene(self) -> Result<Scene, Error> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.scene),
        }
    }

    fn fill_gradient(
        &mut self,
        path: &BezPath,
        kind: EmblemGradientKind,
        spread: Spread,
        stops: &[Stop],
        matrix: Matrix,
    ) -> Result<(), Error> {
        let colors: Vec<ColorStop> = stops
            .iter()
            .map(|stop| ColorStop::from((stop.offset, straight_color(stop.color))))
            .collect();

        let (gradient, brush_to_graphic) = match kind {
            EmblemGradientKind::Linear => (
                Gradient::new_linear(Point::ZERO, Point::new(1.0, 0.0)),
                linear_brush_transform(matrix)?,
            ),
            EmblemGradientKind::Radial => {
                let inverse = matrix.inverse().ok_or(Error::InvalidGradientMatrix)?;
                (
                    Gradient::new_radial(Point::ZERO, 1.0),
                    matrix_to_affine(inverse)?,
                )
            }
        };

        let extend = match spread {
            Spread::None | Spread::Pad => Extend::Pad,
            Spread::Reflect => Extend::Reflect,
            Spread::Repeat => Extend::Repeat,
        };
        let gradient = gradient
            .with_stops(colors.as_slice())
            .with_extend(extend)
            .with_interpolation_alpha_space(InterpolationAlphaSpace::Premultiplied);

        let clipped = spread == Spread::None;
        if clipped {
            self.push_none_spread_clip(kind, path, brush_to_graphic);
        }
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &gradient,
            Some(brush_to_graphic),
            path,
        );
        if clipped {
            self.scene.pop_layer();
        }
        Ok(())
    }

    fn push_none_spread_clip(
        &mut self,
        kind: EmblemGradientKind,
        path: &BezPath,
        brush_to_graphic: Affine,
    ) {
        match kind {
            EmblemGradientKind::Linear => {
                let bounds = path.bounding_box();
                let graphic_to_brush = brush_to_graphic.inverse();
                let corners = [
                    Point::new(bounds.x0, bounds.y0),
                    Point::new(bounds.x1, bounds.y0),
                    Point::new(bounds.x1, bounds.y1),
                    Point::new(bounds.x0, bounds.y1),
                ];
                let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
                for corner in corners {
                    let y = (graphic_to_brush * corner).y;
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
                let strip = Rect::new(0.0, min_y, 1.0, max_y);
                self.scene
                    .push_clip_layer(Fill::NonZero, brush_to_graphic, &strip);
            }
            EmblemGradientKind::Radial => {
                let disc = Circle::new(Point::ZERO, 1.0);
                self.scene
                    .push_clip_layer(Fill::NonZero, brush_to_graphic, &disc);
            }
        }
    }
}

impl Default for VelloSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EmblemSink for VelloSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path
            .quad_to((f64::from(cx), f64::from(cy)), (f64::from(x), f64::from(y)));
    }

    fn cube_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.path.curve_to(
            (f64::from(c1x), f64::from(c1y)),
            (f64::from(c2x), f64::from(c2y)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }

    fn fill(&mut self, paint: &Paint) {
        let path = core::mem::take(&mut self.path);
        if path.elements().is_empty() || self.error.is_some() {
            return;
        }

        let result = match paint {
            Paint::Flat(color) => {
                self.scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    straight_color(*color),
                    None,
                    &path,
                );
                Ok(())
            }
            Paint::Gradient {
                kind,
                spread,
                stops,
                matrix,
            } => self.fill_gradient(&path, *kind, *spread, stops, *matrix),
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}

/// Decode one IconVG file into a clipped vello scene fragment.
pub fn decode(file: &[u8], palette: &Palette, host: Host) -> Result<Graphic, Error> {
    let (metadata, bytecode_at) = emblem::decode_metadata(file)?;
    let mut sink = VelloSink::new();
    emblem::execute(file, bytecode_at, palette, host, &mut sink)?;
    let content = sink.into_scene()?;

    let view_box = metadata.view_box;
    let clip = Rect::new(
        f64::from(view_box.min_x),
        f64::from(view_box.min_y),
        f64::from(view_box.max_x),
        f64::from(view_box.max_y),
    );
    let mut scene = Scene::new();
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &clip);
    scene.append(&content, None);
    scene.pop_layer();

    Ok(Graphic { scene, view_box })
}

fn straight_color(color: Rgba) -> Color {
    if color.a == 0 {
        return Color::from_rgba8(0, 0, 0, 0);
    }
    let alpha = u32::from(color.a);
    let straight = |channel: u8| ((u32::from(channel) * 255 + alpha / 2) / alpha).min(255) as u8;
    Color::from_rgba8(
        straight(color.r),
        straight(color.g),
        straight(color.b),
        color.a,
    )
}

fn linear_brush_transform(matrix: Matrix) -> Result<Affine, Error> {
    let [a, b, c, _, _, _] = matrix.0.map(f64::from);
    let norm_squared = a * a + b * b;
    if !norm_squared.is_finite() || norm_squared == 0.0 || !c.is_finite() {
        return Err(Error::InvalidGradientMatrix);
    }

    let origin = (-a * c / norm_squared, -b * c / norm_squared);
    let along = (a / norm_squared, b / norm_squared);
    let across = (-b / norm_squared, a / norm_squared);
    let affine = Affine::new([along.0, along.1, across.0, across.1, origin.0, origin.1]);
    finite_affine(affine)
}

fn matrix_to_affine(matrix: Matrix) -> Result<Affine, Error> {
    let [a, b, c, d, e, f] = matrix.0.map(f64::from);
    finite_affine(Affine::new([a, d, b, e, c, f]))
}

fn finite_affine(affine: Affine) -> Result<Affine, Error> {
    if affine
        .as_coeffs()
        .iter()
        .all(|coefficient| coefficient.is_finite())
    {
        Ok(affine)
    } else {
        Err(Error::InvalidGradientMatrix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTION_INFO: [u8; 36] = [
        0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33,
        0x59, 0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75,
        0x34, 0x7d, 0x75, 0x7d, 0x6d, 0x88,
    ];

    fn rectangle(sink: &mut VelloSink) {
        sink.move_to(-2.0, -2.0);
        sink.line_to(2.0, -2.0);
        sink.line_to(2.0, 2.0);
        sink.line_to(-2.0, 2.0);
        sink.close();
    }

    fn stops() -> Vec<Stop> {
        vec![
            Stop {
                offset: 0.0,
                color: Rgba::new(0x80, 0, 0, 0x80),
            },
            Stop {
                offset: 1.0,
                color: Rgba::new(0, 0, 0xFF, 0xFF),
            },
        ]
    }

    #[test]
    fn path_segments_become_one_nonzero_vello_fill() {
        let mut sink = VelloSink::new();
        sink.move_to(0.0, 0.0);
        sink.line_to(1.0, 0.0);
        sink.quad_to(2.0, 0.0, 2.0, 1.0);
        sink.cube_to(2.0, 2.0, 1.0, 2.0, 0.0, 1.0);
        sink.close();
        sink.fill(&Paint::Flat(Rgba::new(0x20, 0x40, 0x60, 0xFF)));

        let scene = sink.into_scene().unwrap();
        let encoding = scene.encoding();
        assert_eq!(encoding.n_paths, 1);
        assert_eq!(encoding.draw_tags.len(), 1);
        assert_eq!(encoding.styles.len(), 1);
        // Vello encoding style bits: bit 31 is stroke, bit 30 is even-odd.
        assert_eq!(encoding.styles[0].flags_and_miter_limit & 0xC000_0000, 0);
    }

    #[test]
    fn action_info_decodes_with_its_viewbox_clip() {
        let graphic = decode(&ACTION_INFO, &Palette::default(), Host::default()).unwrap();
        assert_eq!(graphic.view_box().min_x, -24.0);
        assert_eq!(graphic.view_box().max_x, 24.0);
        assert_eq!(graphic.scene().encoding().n_clips, 2);
        assert!(graphic.scene().encoding().n_paths >= 2);
    }

    #[test]
    fn palette_swap_changes_the_vello_brush_not_the_iconvg_bytes() {
        let file = crate::derive(b"vello palette receipt").unwrap();
        let red = Palette::new([Rgba::new(0xFF, 0, 0, 0xFF); 64]).unwrap();
        let blue = Palette::new([Rgba::new(0, 0, 0xFF, 0xFF); 64]).unwrap();
        let host = Host {
            height: 64.0,
            ..Host::default()
        };

        let red_scene = decode(&file, &red, host).unwrap();
        let blue_scene = decode(&file, &blue, host).unwrap();
        assert_ne!(
            red_scene.scene().encoding().draw_data,
            blue_scene.scene().encoding().draw_data
        );
    }

    #[test]
    fn every_gradient_spread_lowers_and_none_adds_a_clip() {
        for spread in [Spread::None, Spread::Pad, Spread::Reflect, Spread::Repeat] {
            let mut sink = VelloSink::new();
            rectangle(&mut sink);
            sink.fill(&Paint::Gradient {
                kind: EmblemGradientKind::Linear,
                spread,
                stops: stops(),
                matrix: Matrix([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            });
            let scene = sink.into_scene().unwrap();
            assert_eq!(
                scene.encoding().n_clips,
                2 * u32::from(spread == Spread::None)
            );
            assert!(
                scene
                    .encoding()
                    .styles
                    .iter()
                    .all(|style| style.flags_and_miter_limit & 0xC000_0000 == 0)
            );
        }
    }

    #[test]
    fn radial_gradient_uses_the_inverse_effective_matrix() {
        let mut sink = VelloSink::new();
        rectangle(&mut sink);
        sink.fill(&Paint::Gradient {
            kind: EmblemGradientKind::Radial,
            spread: Spread::Pad,
            stops: stops(),
            matrix: Matrix::IDENTITY,
        });
        let scene = sink.into_scene().unwrap();
        assert_eq!(scene.encoding().draw_tags.len(), 1);
    }

    #[test]
    fn singular_radial_gradient_is_reported() {
        let mut sink = VelloSink::new();
        rectangle(&mut sink);
        sink.fill(&Paint::Gradient {
            kind: EmblemGradientKind::Radial,
            spread: Spread::Pad,
            stops: stops(),
            matrix: Matrix([0.0; 6]),
        });
        assert!(matches!(
            sink.into_scene(),
            Err(Error::InvalidGradientMatrix)
        ));
    }

    #[test]
    fn premultiplied_emblem_color_becomes_straight_peniko_color() {
        assert_eq!(
            straight_color(Rgba::new(0x80, 0, 0, 0x80)),
            Color::from_rgba8(0xFF, 0, 0, 0x80)
        );
        assert_eq!(
            straight_color(Rgba::TRANSPARENT),
            Color::from_rgba8(0, 0, 0, 0)
        );
    }
}
