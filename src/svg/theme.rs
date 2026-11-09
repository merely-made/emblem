// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Recovering which colours came from palette slots, without touching the VM.
//!
//! The VM resolves a slot to its colour before a [`crate::Sink`] sees it, so a
//! sink cannot tell a slot from a literal. Executing twice tells them apart:
//! once with the caller's palette `P`, once with a probe palette `P'` of 64
//! distinct opaque colours where every `P'[i]` differs from `P[i]`. A colour is
//! slot `i` exactly when the first pass gives `P[i]` and the second `P'[i]`. A
//! literal is the same in both passes, so it can never match both; a blend or
//! an alpha-scaled slot changes between passes without landing on `P'[i]`, so
//! it stays literal too.

use super::{SvgError, SvgSink};
use crate::{Call, Host, Paint, Palette, Recorder, Rgba, Sink};

/// Drive `sink` from `file`, telling it which colours are palette slots.
pub(super) fn execute(
    file: &[u8],
    start: usize,
    palette: &Palette,
    host: Host,
    sink: &mut SvgSink,
) -> Result<(), SvgError> {
    let probe = probe(palette);
    let mut first = Recorder::default();
    crate::execute(file, start, palette, host, &mut first)?;
    let mut second = Recorder::default();
    crate::execute(file, start, &probe, host, &mut second)?;

    // Palettes never steer control flow, so the passes line up call for call.
    // Should they not, every colour simply stays literal.
    let aligned = first.calls.len() == second.calls.len();
    for (i, call) in first.calls.iter().enumerate() {
        match *call {
            Call::MoveTo(x, y) => sink.move_to(x, y),
            Call::LineTo(x, y) => sink.line_to(x, y),
            Call::QuadTo(cx, cy, x, y) => sink.quad_to(cx, cy, x, y),
            Call::CubeTo(ax, ay, bx, by, x, y) => sink.cube_to(ax, ay, bx, by, x, y),
            Call::Close => sink.close(),
            Call::Fill(ref paint) => {
                let slots = match second.calls.get(i) {
                    Some(Call::Fill(other)) if aligned => slots(paint, other, palette, &probe),
                    _ => Vec::new(),
                };
                sink.fill_slotted(paint, &slots);
            },
        }
    }
    Ok(())
}

/// 64 distinct opaque colours, each different from the caller's entry.
fn probe(palette: &Palette) -> Palette {
    let mut colors = [Rgba::TRANSPARENT; 64];
    for (i, (color, theirs)) in colors.iter_mut().zip(palette.0.iter()).enumerate() {
        // Each index owns the red values 4i..4i+3, so no two entries collide.
        let red = (i * 4) as u8;
        let first = Rgba::new(red + 1, 0x5a, 0xa5, 0xff);
        *color = if first == *theirs {
            Rgba::new(red + 2, 0x5a, 0xa5, 0xff)
        } else {
            first
        };
    }
    Palette(colors)
}

/// The slot behind each colour of a paint: one entry for a flat fill, one per
/// stop for a gradient.
fn slots(first: &Paint, second: &Paint, palette: &Palette, probe: &Palette) -> Vec<Option<u8>> {
    let slot = |color: Rgba, probed: Rgba| {
        let i = probe.0.iter().position(|p| *p == probed)?;
        (palette.0[i] == color).then_some(i as u8)
    };
    match (first, second) {
        (Paint::Flat(a), Paint::Flat(b)) => vec![slot(*a, *b)],
        (Paint::Gradient { stops: a, .. }, Paint::Gradient { stops: b, .. })
            if a.len() == b.len() =>
        {
            a.iter()
                .zip(b)
                .map(|(a, b)| slot(a.color, b.color))
                .collect()
        },
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_differs_from_every_palette_and_from_itself() {
        let mut colors = [Rgba::new(0, 0, 0, 0xff); 64];
        // A palette that already holds the probe's first choice for slot 5.
        colors[5] = Rgba::new(21, 0x5a, 0xa5, 0xff);
        let palette = Palette(colors);
        let probe = probe(&palette);
        for (i, color) in probe.0.iter().enumerate() {
            assert_ne!(*color, palette.0[i], "slot {i}");
            assert_eq!(probe.0.iter().filter(|c| *c == color).count(), 1);
        }
    }
}
