// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use super::*;

impl<'a> Vm<'a> {
    pub(super) fn geometry<S: Sink>(
        &mut self,
        bytes: &[u8],
        opcode: u8,
        sink: &mut S,
    ) -> Result<()> {
        let (reps, header) = oplen::repetition_count(bytes, usize::from(opcode & 0x0F))?;
        let per_rep = match opcode >> 4 {
            0 => 2,
            1 => 4,
            _ => 6,
        };
        let mut at = header;
        for _ in 0..reps {
            let mut points = [(0.0f32, 0.0f32); 3];
            for point in points.iter_mut().take(per_rep / 2) {
                let (x, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += used;
                let (y, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += used;
                *point = self.gftm.apply(x, y);
            }
            self.ensure_open(sink);
            match per_rep {
                2 => {
                    sink.line_to(points[0].0, points[0].1);
                    self.pen = points[0];
                },
                4 => {
                    sink.quad_to(points[0].0, points[0].1, points[1].0, points[1].1);
                    self.pen = points[1];
                },
                _ => {
                    sink.cube_to(
                        points[0].0,
                        points[0].1,
                        points[1].0,
                        points[1].1,
                        points[2].0,
                        points[2].1,
                    );
                    self.pen = points[2];
                },
            }
        }
        Ok(())
    }

    pub(super) fn ellipse_or_parallelogram<S: Sink>(
        &mut self,
        bytes: &[u8],
        opcode: u8,
        sink: &mut S,
    ) -> Result<()> {
        let (b, c) = {
            let mut at = 1;
            let mut read = || -> Result<(f32, f32)> {
                let (x, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += used;
                let (y, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += used;
                Ok(self.gftm.apply(x, y))
            };
            (read()?, read()?)
        };

        let a = self.pen;
        // The implied fourth corner of the parallelogram.
        let d = (a.0 - b.0 + c.0, a.1 - b.1 + c.1);
        self.ensure_open(sink);

        if opcode == 0x34 {
            // Parallelogram: four line segments back to A, pen unchanged.
            sink.line_to(b.0, b.1);
            sink.line_to(c.0, c.1);
            sink.line_to(d.0, d.1);
            sink.line_to(a.0, a.1);
            return Ok(());
        }

        // Centre and the two axis vectors the four arcs are built from.
        let x = ((a.0 + c.0) * 0.5, (a.1 + c.1) * 0.5);
        let r = (b.0 - x.0, b.1 - x.1);
        let s = (c.0 - x.0, c.1 - x.1);
        let k = ELLIPSE_K;

        // The eight off-curve control points, exactly as the spec lists them.
        let a_plus = (a.0 + k * r.0, a.1 + k * r.1);
        let b_minus = (b.0 - k * s.0, b.1 - k * s.1);
        let b_plus = (b.0 + k * s.0, b.1 + k * s.1);
        let c_minus = (c.0 + k * r.0, c.1 + k * r.1);
        let c_plus = (c.0 - k * r.0, c.1 - k * r.1);
        let d_minus = (d.0 + k * s.0, d.1 + k * s.1);
        let d_plus = (d.0 - k * s.0, d.1 - k * s.1);
        let a_minus = (a.0 - k * r.0, a.1 - k * r.1);

        let quarters = opcode - 0x30 + 1;
        let arcs = [
            (a_plus, b_minus, b),
            (b_plus, c_minus, c),
            (c_plus, d_minus, d),
            (d_plus, a_minus, a),
        ];
        for (c1, c2, end) in arcs.iter().take(usize::from(quarters)) {
            sink.cube_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
            self.pen = *end;
        }
        Ok(())
    }

    pub(super) fn close_path_move_to<S: Sink>(&mut self, bytes: &[u8], sink: &mut S) -> Result<()> {
        let mut at = 1;
        let (x, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
        at += used;
        let (y, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
        let _ = used;
        let point = self.gftm.apply(x, y);

        self.close_path(sink);
        sink.move_to(point.0, point.1);
        self.pen = point;
        self.subpath_start = point;
        self.open = true;
        self.pending = true;
        Ok(())
    }

    /// Close the current subpath and add it to the pending list.
    pub(super) fn close_path<S: Sink>(&mut self, sink: &mut S) {
        if self.open {
            sink.close();
            self.open = false;
            self.pending = true;
        }
    }

    /// Open a subpath at the pen if none is open.
    ///
    /// The Current Path "is initially empty but starts at the origin (0, 0)",
    /// so the first geometry op needs a `move_to` the sink was never given.
    pub(super) fn ensure_open<S: Sink>(&mut self, sink: &mut S) {
        if !self.open {
            sink.move_to(self.pen.0, self.pen.1);
            self.subpath_start = self.pen;
            self.open = true;
            self.pending = true;
        }
    }

    pub(super) fn reserved_line_to<S: Sink>(&mut self, bytes: &[u8], sink: &mut S) -> Result<()> {
        // Extra Data first, then the coordinate pair to line to.
        let (extra, used) = num::natural(bytes.get(1..).ok_or(Error::Truncated)?)?;
        let mut at = 1 + used + extra as usize;
        let (x, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
        at += used;
        let (y, _) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
        let point = self.gftm.apply(x, y);
        self.ensure_open(sink);
        sink.line_to(point.0, point.1);
        self.pen = point;
        Ok(())
    }
}
