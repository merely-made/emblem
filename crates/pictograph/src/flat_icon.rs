// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Flat IconVG lowered to portable paint-list paths for Canvas and icon packaging.
//!
//! Canvas returns a netrender [`Scene`](netrender::Scene), so it cannot append the raw vello
//! fragment exposed by pictograph's optional D2 bridge. The paint list already carries arbitrary
//! paths and netrender fills them with non-zero winding. This small sink keeps Canvas device-free
//! while preserving IconVG's fill rule at the renderer boundary.

use emblem::{Host, Paint, Palette, Rgba, Sink};
use paint_list_api::{
    ColorF, CommonPlacement, LayoutPoint, LayoutRect, PaintCmd, PathCommand, PathData, PathItem,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Iconvg(emblem::Error),
    EmptyViewBox,
    UnsupportedGradient,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Iconvg(error) => write!(f, "{error}"),
            Self::EmptyViewBox => {
                f.write_str("IconVG view box must have positive width and height")
            },
            Self::UnsupportedGradient => f.write_str("flat IconVG lowering cannot paint gradients"),
        }
    }
}

impl std::error::Error for Error {}

impl From<emblem::Error> for Error {
    fn from(error: emblem::Error) -> Self {
        Self::Iconvg(error)
    }
}

/// Decode one generated face into screen-space paint commands bounded by `target`.
pub(crate) fn commands(
    file: &[u8],
    palette: &Palette,
    host_height: f32,
    target: LayoutRect,
) -> Result<Vec<PaintCmd>, Error> {
    let (metadata, bytecode_at) = emblem::decode_metadata(file)?;
    let view_box = metadata.view_box;
    let source_width = view_box.max_x - view_box.min_x;
    let source_height = view_box.max_y - view_box.min_y;
    if source_width <= 0.0 || source_height <= 0.0 {
        return Err(Error::EmptyViewBox);
    }

    let target_width = target.max.x - target.min.x;
    let target_height = target.max.y - target.min.y;
    let scale = (target_width / source_width).min(target_height / source_height);
    let source_center_x = (view_box.min_x + view_box.max_x) * 0.5;
    let source_center_y = (view_box.min_y + view_box.max_y) * 0.5;
    let target_center_x = (target.min.x + target.max.x) * 0.5;
    let target_center_y = (target.min.y + target.max.y) * 0.5;
    let mut sink = PaintListSink::new(
        target,
        scale,
        target_center_x - source_center_x * scale,
        target_center_y - source_center_y * scale,
    );
    emblem::execute(
        file,
        bytecode_at,
        palette,
        Host {
            features: 0,
            height: host_height,
        },
        &mut sink,
    )?;
    sink.finish()
}

struct PaintListSink {
    placement: CommonPlacement,
    path: Vec<PathCommand>,
    commands: Vec<PaintCmd>,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    unsupported_gradient: bool,
}

impl PaintListSink {
    fn new(bounds: LayoutRect, scale: f32, offset_x: f32, offset_y: f32) -> Self {
        Self {
            placement: CommonPlacement::new(bounds),
            path: Vec::new(),
            commands: Vec::new(),
            scale,
            offset_x,
            offset_y,
            unsupported_gradient: false,
        }
    }

    fn point(&self, x: f32, y: f32) -> LayoutPoint {
        LayoutPoint::new(
            x * self.scale + self.offset_x,
            y * self.scale + self.offset_y,
        )
    }

    fn finish(self) -> Result<Vec<PaintCmd>, Error> {
        if self.unsupported_gradient {
            Err(Error::UnsupportedGradient)
        } else {
            Ok(self.commands)
        }
    }
}

impl Sink for PaintListSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.push(PathCommand::MoveTo(self.point(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.push(PathCommand::LineTo(self.point(x, y)));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.push(PathCommand::QuadTo {
            control: self.point(cx, cy),
            to: self.point(x, y),
        });
    }

    fn cube_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.path.push(PathCommand::CurveTo {
            control1: self.point(c1x, c1y),
            control2: self.point(c2x, c2y),
            to: self.point(x, y),
        });
    }

    fn close(&mut self) {
        self.path.push(PathCommand::Close);
    }

    fn fill(&mut self, paint: &Paint) {
        let path = core::mem::take(&mut self.path);
        if path.is_empty() {
            return;
        }
        let Paint::Flat(color) = paint else {
            self.unsupported_gradient = true;
            return;
        };
        self.commands.push(PaintCmd::DrawPath(PathItem {
            placement: self.placement,
            path: PathData { commands: path },
            fill: Some(straight_color(*color)),
            stroke: None,
        }));
    }
}

fn straight_color(color: Rgba) -> ColorF {
    if color.a == 0 {
        return ColorF::TRANSPARENT;
    }
    let alpha = u32::from(color.a);
    let straight = |channel: u8| ((u32::from(channel) * 255 + alpha / 2) / alpha).min(255) as u8;
    ColorF::new(
        f32::from(straight(color.r)) / 255.0,
        f32::from(straight(color.g)) / 255.0,
        f32::from(straight(color.b)) / 255.0,
        f32::from(color.a) / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(side: f32) -> LayoutRect {
        LayoutRect::new(LayoutPoint::new(0.0, 0.0), LayoutPoint::new(side, side))
    }

    fn solid(color: [u8; 4]) -> Palette {
        crate::palette_from_rgba([color; 8])
    }

    fn fills(commands: &[PaintCmd]) -> Vec<ColorF> {
        commands
            .iter()
            .filter_map(|command| match command {
                PaintCmd::DrawPath(path) => path.fill,
                _ => None,
            })
            .collect()
    }

    #[test]
    fn one_byte_vector_recolors_at_decode_time() {
        let file = crate::derive(b"mere://derived-face/recolor").unwrap();
        let red = commands(&file, &solid([255, 0, 0, 255]), 64.0, bounds(64.0)).unwrap();
        let blue = commands(&file, &solid([0, 0, 255, 255]), 64.0, bounds(64.0)).unwrap();

        assert!(!fills(&red).is_empty());
        assert!(
            fills(&red)
                .iter()
                .all(|fill| *fill == ColorF::new(1.0, 0.0, 0.0, 1.0))
        );
        assert!(
            fills(&blue)
                .iter()
                .all(|fill| *fill == ColorF::new(0.0, 0.0, 1.0, 1.0))
        );
    }

    #[test]
    fn host_height_selects_the_embedded_lod_arm() {
        let file = crate::derive(b"mere://derived-face/lod").unwrap();
        let palette = Palette::default();
        let small = commands(&file, &palette, 16.0, bounds(64.0)).unwrap();
        let large = commands(&file, &palette, 64.0, bounds(64.0)).unwrap();

        assert_eq!(small.len(), 1, "the small arm is one bold fill");
        assert!(!large.is_empty());
        assert_ne!(
            format!("{:?}", small),
            format!("{:?}", large),
            "the two LOD arms lower to different paths"
        );
    }
}
