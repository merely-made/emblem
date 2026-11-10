// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The text arm: what a face draws below [`TEXT_THRESHOLD`](crate::TEXT_THRESHOLD),
//! at the height of a line of text, a tab or a list row.
//!
//! At 10 px a 3×3 cell is 3 px: texture, not shape. So the text arm drops the
//! grid and draws one mark filling the box, read from the face's existing
//! parameters only:
//!
//! - the **plate**: a rounded square for [`Form::Block`], a diamond for
//!   [`Form::Diamond`], in the primary entry;
//! - one **figure** in the accent entry, chosen by symmetry: a vertical bar for
//!   [`Symmetry::MirrorX`], a dot for [`Symmetry::MirrorBoth`], a diagonal for
//!   [`Symmetry::Rotate180`].
//!
//! That is 8 primaries × 7 accents × 3 figures × 2 plates = 336 marks. It says
//! "that node's colour and family", not which node; the name always sits
//! beside it. Every coordinate is an integer in `[-30, 30]`, one byte each.

use emblem::encode::{EncodeError, Writer};

use crate::{Form, Params, Symmetry, slot_for};

/// The plate's half-width, matching the grid's extent.
const EDGE: f32 = 30.0;
/// The rounded square's corner radius.
const CORNER: f32 = 10.0;

/// Draw the text arm's plate and figure into `arm`.
pub(crate) fn draw(arm: &mut Writer, params: &Params) -> Result<(), EncodeError> {
    match params.form {
        Form::Block => rounded_square(arm, EDGE, CORNER)?,
        Form::Diamond => {
            arm.move_to(0.0, -EDGE)?;
            // A - B + C lands on (-EDGE, 0), the diamond's fourth point.
            arm.parallelogram((EDGE, 0.0), (0.0, EDGE))?;
        },
    }
    arm.fill_flat(slot_for(params.primary))?;

    match params.symmetry {
        Symmetry::MirrorX => {
            arm.move_to(-6.0, -20.0)?;
            arm.parallelogram((6.0, -20.0), (6.0, 20.0))?;
        },
        Symmetry::MirrorBoth => {
            // A full circle of radius 10 about the centre, as four quarters.
            arm.move_to(0.0, -10.0)?;
            arm.ellipse(4, (-10.0, 0.0), (0.0, 10.0))?;
        },
        Symmetry::Rotate180 => {
            // A band along the falling diagonal; A - B + C is (-10, 18). Its
            // corners keep |x| + |y| <= 28, inside the diamond plate too.
            arm.move_to(-18.0, 10.0)?;
            arm.parallelogram((10.0, -18.0), (18.0, -10.0))?;
        },
    }
    arm.fill_flat(slot_for(params.accent))
}

/// A rounded square whose corners are quadratic curves with
/// the corner itself as control point.
pub(crate) fn rounded_square(arm: &mut Writer, edge: f32, corner: f32) -> Result<(), EncodeError> {
    let (e, r) = (edge, edge - corner);
    arm.move_to(-r, -e)?;
    arm.line_to(&[(r, -e)])?;
    arm.quad_to(&[((e, -e), (e, -r))])?;
    arm.line_to(&[(e, r)])?;
    arm.quad_to(&[((e, e), (r, e))])?;
    arm.line_to(&[(-r, e)])?;
    arm.quad_to(&[((-e, e), (-e, r))])?;
    arm.line_to(&[(-e, -r)])?;
    arm.quad_to(&[((-e, -e), (-r, -e))])
}
