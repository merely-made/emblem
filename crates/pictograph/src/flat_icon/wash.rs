// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use emblem::{GradientKind, Host, Paint, Palette, Spread};
use paint_list_api::{
    ColorF, CommonPlacement, ExtendMode, GradientStop, LayoutPoint, LayoutRect, LayoutSize,
    PaintCmd, PathCommand, RadialGradientItem, RadialGradientPayload,
};

use super::{Error, PaintListSink};

/// Lower wash bytes in their own coordinate space. The canvas projects the
/// paths and the gradient rectangles together.
pub(crate) fn wash_commands(
    file: &[u8],
    palette: &Palette,
    host_height: f32,
) -> Result<Vec<PaintCmd>, Error> {
    let (metadata, bytecode_at) = emblem::decode_metadata(file)?;
    let view = metadata.view_box;
    let bounds = LayoutRect::new(
        LayoutPoint::new(view.min_x, view.min_y),
        LayoutPoint::new(view.max_x, view.max_y),
    );
    let mut sink = PaintListSink::new(bounds, 1.0, 0.0, 0.0);
    sink.allow_wash_gradients = true;
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

/// Wash gradients fill one axis-aligned rectangle, with a diagonal brush
/// matrix. These are the shapes the portable radial primitive represents.
pub(super) fn radial_command(paint: &Paint, path: &[PathCommand]) -> Option<PaintCmd> {
    let Paint::Gradient {
        kind: GradientKind::Radial,
        spread: Spread::Pad,
        stops,
        matrix,
    } = paint
    else {
        return None;
    };
    let [
        PathCommand::MoveTo(top_left),
        PathCommand::LineTo(top_right),
        PathCommand::LineTo(bottom_right),
        PathCommand::LineTo(bottom_left),
        PathCommand::LineTo(back),
        PathCommand::Close,
    ] = path
    else {
        return None;
    };
    if top_left != back
        || top_left.y != top_right.y
        || top_right.x != bottom_right.x
        || bottom_right.y != bottom_left.y
        // The VM computes the implied fourth vertex with f32 arithmetic.
        || (bottom_left.x - top_left.x).abs() > 8.0 * f32::EPSILON * 32.0
        || top_left.x > bottom_right.x
        || top_left.y > bottom_right.y
    {
        return None;
    }
    let [a, b, c, d, e, f] = matrix.0;
    if b != 0.0 || d != 0.0 || a <= 0.0 || e <= 0.0 {
        return None;
    }
    let bounds = LayoutRect::new(*top_left, *bottom_right);
    Some(PaintCmd::DrawRadialGradient(RadialGradientItem {
        placement: CommonPlacement::new(bounds),
        gradient: RadialGradientPayload {
            center: LayoutPoint::new(-c / a, -f / e),
            radius: LayoutSize::new(a.recip(), e.recip()),
            extend_mode: ExtendMode::Clamp,
            stops: stops
                .iter()
                .map(|stop| GradientStop {
                    offset: stop.offset,
                    // The pinned paint-list gradient translator copies stop
                    // channels directly into premultiplied SceneGradient stops.
                    color: ColorF::new(
                        f32::from(stop.color.r) / 255.0,
                        f32::from(stop.color.g) / 255.0,
                        f32::from(stop.color.b) / 255.0,
                        f32::from(stop.color.a) / 255.0,
                    ),
                })
                .collect(),
        },
        tile_size: bounds.size(),
        tile_spacing: LayoutSize::zero(),
    }))
}
