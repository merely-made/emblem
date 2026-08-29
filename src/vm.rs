// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The IconVG bytecode virtual machine.
//!
//! A register-based machine over the four opcode categories: geometry,
//! register, fill, and reserved. It walks a graphic's bytecode and drives a
//! [`Sink`] with path segments and fills.
//!
//! ## What the machine holds
//!
//! Program Counter, End Of Bytecode, and Global Return Address for control
//! flow; a 64-entry register file and a selector `SEL` indexing it; a global
//! alpha and a forward transformation matrix (with its inverse, needed by
//! gradients); a pen position; and the paths accumulated since the last fill.
//!
//! ## The two ceilings that are not arbitrary
//!
//! **Calls do not nest.** "IconVG bytecode calls cannot nest. The maximum call
//! stack depth is one." A Call while the GRA is non-zero is invalid, and that
//! is what bounds recursion -- there is no depth limit to tune, because the
//! format does not permit depth.
//!
//! **Jumps only go forward.** Jump ops "propel the PC forward (never
//! backward)", so bytecode cannot loop. Together those two rules mean a
//! well-formed graphic always terminates, which is why this VM needs no fuel
//! counter.

use crate::color::{Palette, Registers, Rgba};
use crate::oplen::{self, op_length};
use crate::sink::{GradientKind, Matrix, Paint, Sink, Spread, Stop};
use crate::{Error, Result, num};

/// The magic constant that turns three points into an ellipse, from
/// ["Three Points (Two Opposing) Define an
/// Ellipse"](https://nigeltao.github.io/blog/2020/three-points-define-ellipse.html).
const ELLIPSE_K: f32 = 0.551_784_77;

/// End Of Bytecode's initial value.
const EOB_MAX: u64 = u64::MAX;

/// What the host rasterizer can do, which the bytecode may branch on.
#[derive(Clone, Copy, Debug)]
pub struct Host {
    /// The `FeaturesImplemented` bitmask. All bits are currently reserved, so
    /// zero is the correct answer for any rasterizer built against this
    /// version of the specification.
    pub features: u32,
    /// The rasterization's height in pixels, which Level Of Detail jumps
    /// branch on. A graphic can then ship a simple version for small sizes and
    /// a detailed one for large.
    pub height: f32,
}

impl Default for Host {
    fn default() -> Self {
        // Zero features is not a placeholder: every feature bit is reserved by
        // this version of the spec, so a conforming rasterizer implements none.
        Self {
            features: 0,
            height: 0.0,
        }
    }
}

/// Execute a graphic's bytecode, driving `sink`.
///
/// `file` is the whole IconVG file, because Call ops address segments by
/// absolute file offset. `start` is where the bytecode begins, which
/// [`crate::decode_metadata`] returns.
pub fn execute<S: Sink>(
    file: &[u8],
    start: usize,
    palette: &Palette,
    host: Host,
    sink: &mut S,
) -> Result<()> {
    Vm::new(file, start, palette, host).run(sink)
}

struct Vm<'a> {
    file: &'a [u8],
    palette: &'a Palette,
    host: Host,

    pc: usize,
    eob: u64,
    gra: u64,

    sel: u8,
    regs: Registers,

    alpha: f32,
    gftm: Matrix,
    gbtm: Matrix,

    pen: (f32, f32),
    subpath_start: (f32, f32),
    /// Whether anything has been emitted into the current subpath.
    open: bool,
    /// Whether any path is pending a fill.
    pending: bool,
}

impl<'a> Vm<'a> {
    fn new(file: &'a [u8], start: usize, palette: &'a Palette, host: Host) -> Self {
        // SEL comes from the register file rather than a literal here. The
        // specification initialises it to 56, "so that, at the start,
        // REGS[SEL+0 .. SEL+8] can be set to a vector graphic's hard-coded
        // colors while REGS[SEL+8 .. SEL+16] accesses the Custom Palette's
        // first eight colors". This field used to be initialised to 0
        // independently of `Registers::new`, which set 56 correctly and was
        // then never read: every register and fill op addressed a register 56
        // slots away from the one the graphic named, so a fill resolved to the
        // wrong palette entry. Taking the value from `regs` keeps the two from
        // drifting apart again.
        let regs = Registers::new(palette);
        Self {
            file,
            palette,
            host,
            pc: start,
            eob: EOB_MAX,
            gra: 0,
            sel: regs.sel,
            regs,
            alpha: 1.0,
            gftm: Matrix::IDENTITY,
            gbtm: Matrix::IDENTITY,
            pen: (0.0, 0.0),
            subpath_start: (0.0, 0.0),
            open: false,
            pending: false,
        }
    }

    fn run<S: Sink>(&mut self, sink: &mut S) -> Result<()> {
        loop {
            // "The VM executes an implicit Return op when the PC is at the EOB
            // or at the end of the IconVG file."
            if self.pc as u64 >= self.eob || self.pc >= self.file.len() {
                if self.do_return()? {
                    return Ok(());
                }
                continue;
            }
            if self.step(sink)? {
                return Ok(());
            }
        }
    }

    /// The remainder of the file from the PC, bounded by the EOB.
    ///
    /// Bounding matters: "It is invalid for an op's bytes to cross the EOB",
    /// and without the bound a callee could read past its segment into
    /// whatever bytes happen to follow.
    fn here(&self) -> &'a [u8] {
        let end = (self.eob.min(self.file.len() as u64)) as usize;
        &self.file[self.pc..end.max(self.pc)]
    }

    /// Execute one op. Returns `true` when the graphic has ended.
    fn step<S: Sink>(&mut self, sink: &mut S) -> Result<bool> {
        let bytes = self.here();
        let opcode = *bytes.first().ok_or(Error::Truncated)?;
        let low4 = opcode & 0x0F;
        let length = op_length(bytes)?;
        // An op must fit inside the current bytecode extent.
        if length > bytes.len() {
            return Err(Error::Truncated);
        }

        match opcode {
            0x00..=0x2F => self.geometry(bytes, opcode, sink)?,
            0x30..=0x34 => self.ellipse_or_parallelogram(bytes, opcode, sink)?,
            0x35 => self.close_path_move_to(bytes, sink)?,
            0x36 => {
                let delta = *bytes.get(1).ok_or(Error::Truncated)?;
                self.sel = self.sel.wrapping_add(delta) & 0x3F;
            },
            0x37 => {},
            0x38..=0x3A => {
                // A taken jump sets the PC itself.
                if self.jump(bytes, opcode, length)? {
                    return Ok(false);
                }
            },
            0x3B => return self.do_return(),
            0x3C | 0x3D => {
                self.call(bytes, opcode, length)?;
                return Ok(false);
            },
            0x3E..=0x3F => {}, // Reserved: a NOP in this version.
            0x40..=0x7F => self.register_op(bytes, opcode, low4)?,
            // 0xB0..=0xBF falls back to a flat fill, "as if the opcode was in
            // [0x80 ..= 0x8F]. This includes pre-incrementing SEL".
            0x80..=0x8F | 0xB0..=0xBF => self.fill_flat(low4, sink),
            0x90..=0xAF => self.fill_gradient(bytes, opcode, low4, sink)?,
            0xC0..=0xDF => self.reserved_line_to(bytes, sink)?,
            0xE0..=0xFF => {}, // Reserved: a NOP.
        }

        self.pc += length;
        Ok(false)
    }

    // --- geometry ------------------------------------------------------

    fn geometry<S: Sink>(&mut self, bytes: &[u8], opcode: u8, sink: &mut S) -> Result<()> {
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

    fn ellipse_or_parallelogram<S: Sink>(
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

    fn close_path_move_to<S: Sink>(&mut self, bytes: &[u8], sink: &mut S) -> Result<()> {
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
    fn close_path<S: Sink>(&mut self, sink: &mut S) {
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
    fn ensure_open<S: Sink>(&mut self, sink: &mut S) {
        if !self.open {
            sink.move_to(self.pen.0, self.pen.1);
            self.subpath_start = self.pen;
            self.open = true;
            self.pending = true;
        }
    }

    fn reserved_line_to<S: Sink>(&mut self, bytes: &[u8], sink: &mut S) -> Result<()> {
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

    // --- control flow --------------------------------------------------

    /// Returns `true` when the jump was taken (the PC is already updated).
    fn jump(&mut self, bytes: &[u8], opcode: u8, length: usize) -> Result<bool> {
        let (count, used) = num::natural(bytes.get(1..).ok_or(Error::Truncated)?)?;
        let take = match opcode {
            0x38 => true,
            0x39 => {
                let (needed, _) = num::natural(bytes.get(1 + used..).ok_or(Error::Truncated)?)?;
                // Taken *unless* the host provides every needed feature.
                (self.host.features & needed) != needed
            },
            _ => {
                let mut at = 1 + used;
                let (lod0, u) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += u;
                let (lod1, _) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                // Taken *unless* the height falls in [LOD0, LOD1).
                !(lod0 <= self.host.height && self.host.height < lod1)
            },
        };
        if !take {
            return Ok(false);
        }

        // Jumps skip whole ops, so walk and measure rather than adding bytes.
        let mut pc = self.pc + length;
        for _ in 0..count {
            let end = (self.eob.min(self.file.len() as u64)) as usize;
            if pc as u64 >= self.eob || pc >= self.file.len() {
                // "It is valid to jump to the EOB or the end of the file
                // exactly", where an implicit Return waits.
                break;
            }
            pc += op_length(&self.file[pc..end.max(pc)])?;
        }
        self.pc = pc;
        Ok(true)
    }

    /// Returns `true` when the graphic has ended.
    fn do_return(&mut self) -> Result<bool> {
        if self.gra == 0 {
            return Ok(true);
        }
        self.pc = self.gra as usize;
        self.eob = EOB_MAX;
        self.alpha = 1.0;
        self.gftm = Matrix::IDENTITY;
        self.gbtm = Matrix::IDENTITY;
        self.gra = 0;
        Ok(false)
    }

    fn call(&mut self, bytes: &[u8], opcode: u8, length: usize) -> Result<()> {
        // "If the GRA is non-zero then the Call op is invalid." This is the
        // whole of the recursion bound: depth one, enforced by the format.
        if self.gra != 0 {
            return Err(Error::NestedCall);
        }

        let mut at = 1;
        if opcode == 0x3D {
            let raw_alpha = *bytes.get(1).ok_or(Error::Truncated)?;
            at = 2;
            let mut matrix = [0.0f32; 6];
            for slot in &mut matrix {
                let (value, used) = num::coordinate(bytes.get(at..).ok_or(Error::Truncated)?)?;
                at += used;
                *slot = value;
            }
            self.alpha = f32::from(raw_alpha) / 255.0;
            self.gftm = Matrix(matrix);
            // A singular forward matrix collapses the graphic; gradients
            // defined against its inverse have no meaning, so refuse rather
            // than emit infinities.
            self.gbtm = self.gftm.inverse().ok_or(Error::SingularMatrix)?;
        }

        let raw = oplen::read_u64(bytes.get(at..).ok_or(Error::Truncated)?)?;
        if oplen::segment_type(raw) != 0x00 {
            // "Segment Types other than 0x00 are reserved and are invalid to
            // Call in this version (there is no fallback behavior)."
            return Err(Error::ReservedSegmentType);
        }

        let (offset, seg_length) = if oplen::is_inline(raw) {
            (self.pc + at + 8, oplen::inline_length(raw))
        } else if raw >> 63 == 0 {
            // Absolute Direct: 31 bits of offset above the packed low half.
            let offset = ((raw >> 33) & 0x7FFF_FFFF) as usize;
            if offset == 0 {
                return Err(Error::BadSegRef);
            }
            (offset, oplen::inline_length(raw))
        } else {
            // Absolute Indirect: 55 middle bits locate 16 bytes holding a
            // 64-bit length then a 64-bit offset.
            let at = ((raw >> 8) & 0x007F_FFFF_FFFF_FFFF) as usize;
            let table = self.file.get(at..at + 16).ok_or(Error::Truncated)?;
            let seg_length = oplen::read_u64(&table[..8])? as usize;
            let offset = oplen::read_u64(&table[8..])? as usize;
            (offset, seg_length)
        };

        let end = offset
            .checked_add(seg_length)
            .ok_or(Error::BadSegRef)?;
        if end > self.file.len() {
            return Err(Error::Truncated);
        }

        self.gra = (self.pc + length) as u64;
        self.pc = offset;
        self.eob = end as u64;
        Ok(())
    }

    // --- registers -----------------------------------------------------

    fn register_op(&mut self, bytes: &[u8], opcode: u8, low4: u8) -> Result<()> {
        let index = usize::from(self.sel.wrapping_add(low4));
        match opcode {
            0x40..=0x4F => {
                let value = u64::from(read_u32(bytes, 1)?);
                self.regs.set(index, value);
                self.decrement_sel_if_low4_zero(low4);
            },
            0x50..=0x5F => {
                let value = u64::from(read_u32(bytes, 1)?) << 32;
                self.regs.set(index, value);
                self.decrement_sel_if_low4_zero(low4);
            },
            0x60..=0x6F => {
                let low = u64::from(read_u32(bytes, 1)?);
                let high = u64::from(read_u32(bytes, 5)?);
                self.regs.set(index, low | (high << 32));
                self.decrement_sel_if_low4_zero(low4);
            },
            _ => {
                // 0x70..=0x7F: decrement SEL first, then fill upwards.
                let count = usize::from(low4) + 2;
                self.sel = self.sel.wrapping_sub(count as u8) & 0x3F;
                for i in 0..count {
                    let low = u64::from(read_u32(bytes, 1 + i * 8)?);
                    let high = u64::from(read_u32(bytes, 5 + i * 8)?);
                    let target = usize::from(self.sel).wrapping_add(i + 1);
                    self.regs.set(target, low | (high << 32));
                }
            },
        }
        Ok(())
    }

    /// "if `LOW4 == 0` then the op also decrements `SEL` by one, after
    /// updating `REGS`."
    fn decrement_sel_if_low4_zero(&mut self, low4: u8) {
        if low4 == 0 {
            self.sel = self.sel.wrapping_sub(1) & 0x3F;
        }
    }

    // --- fills ---------------------------------------------------------

    fn fill_flat<S: Sink>(&mut self, low4: u8, sink: &mut S) {
        if low4 == 0 {
            self.sel = self.sel.wrapping_add(1) & 0x3F;
        }
        let index = usize::from(self.sel.wrapping_add(low4));
        let color = self.apply_alpha(self.regs.color(index, self.palette));
        self.emit_fill(&Paint::Flat(color), sink);
    }

    fn fill_gradient<S: Sink>(
        &mut self,
        bytes: &[u8],
        opcode: u8,
        low4: u8,
        sink: &mut S,
    ) -> Result<()> {
        if low4 == 0 {
            self.sel = self.sel.wrapping_add(1) & 0x3F;
        }
        let config = *bytes.get(1).ok_or(Error::Truncated)?;
        let stop_bits = config & 0x3F;
        if stop_bits == 63 {
            return Err(Error::BadGradient);
        }
        let stop_count = usize::from(stop_bits) + 2;
        let spread = Spread::from_bits(config >> 6);

        let (kind, count) = if opcode < 0xA0 {
            (GradientKind::Linear, 3)
        } else {
            (GradientKind::Radial, 6)
        };
        let mut nominal = [0.0f32; 6];
        for (i, slot) in nominal.iter_mut().enumerate().take(count) {
            let at = 2 + i * 4;
            let (value, _) = num::float32(bytes.get(at..).ok_or(Error::Truncated)?)?;
            *slot = value;
        }

        let base = usize::from(self.sel.wrapping_add(low4));
        let stops: Vec<Stop> = (0..stop_count)
            .map(|i| Stop {
                offset: self.regs.stop_position(base + i),
                color: self.apply_alpha(self.regs.color(base + i, self.palette)),
            })
            .collect();

        let paint = Paint::Gradient {
            kind,
            spread,
            stops,
            matrix: effective_gradient_matrix(&nominal, &self.gbtm),
        };
        self.emit_fill(&paint, sink);
        Ok(())
    }

    /// Scale a colour by the global alpha.
    ///
    /// Premultiplied throughout, so every channel scales -- including RGB.
    /// Scaling only alpha would brighten the colour as it faded.
    fn apply_alpha(&self, color: Rgba) -> Rgba {
        if self.alpha >= 1.0 {
            return color;
        }
        let scale = |channel: u8| (f32::from(channel) * self.alpha).round().clamp(0.0, 255.0) as u8;
        Rgba {
            r: scale(color.r),
            g: scale(color.g),
            b: scale(color.b),
            a: scale(color.a),
        }
    }

    /// ClosePath without moving the pen, then fill and clear the pending list.
    fn emit_fill<S: Sink>(&mut self, paint: &Paint, sink: &mut S) {
        if self.open {
            sink.close();
            self.open = false;
        }
        if self.pending {
            sink.fill(paint);
            self.pending = false;
        }
    }
}

/// `EGM = NGM * GBTM`, mapping graphic space to gradient space.
fn effective_gradient_matrix(nominal: &[f32; 6], gbtm: &Matrix) -> Matrix {
    let [na, nb, nc, nd, ne, nf] = *nominal;
    let [ba, bb, bc, bd, be, bf] = gbtm.0;
    Matrix([
        na * ba + nb * bd,
        na * bb + nb * be,
        na * bc + nb * bf + nc,
        nd * ba + ne * bd,
        nd * bb + ne * be,
        nd * bc + ne * bf + nf,
    ])
}

/// Read a little-endian `u32` at `offset`.
fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes(slice.try_into().expect("checked length")))
}
