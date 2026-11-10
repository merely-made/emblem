// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Authored chrome symbols. Both sizes use the host's ink palette entry.

#[cfg(feature = "canvas")]
mod image;
mod svg;

#[cfg(feature = "canvas")]
pub use image::SymbolImage;

use std::sync::OnceLock;

use emblem::encode::{EncodeError, Writer};
use emblem::{Palette, ViewBox};

use crate::{FIRST_PALETTE_SLOT, LOD_THRESHOLD, PALETTE_SPAN, block, palette_from_rgba};

/// The twelve custom entries shared by faces and symbols.
pub const SYMBOL_PALETTE_SPAN: usize = PALETTE_SPAN as usize + 4;
/// Monochrome chrome and text.
pub const INK_ENTRY: usize = PALETTE_SPAN as usize;
/// Subdued chrome.
pub const MUTED_INK_ENTRY: usize = INK_ENTRY + 1;
/// The host surface.
pub const SURFACE_ENTRY: usize = INK_ENTRY + 2;
/// Reserved for host identity.
pub const HOST_SEAL_ENTRY: usize = INK_ENTRY + 3;

// Advance past the face entries once so slots 8–11 address entries 8–11.
pub(crate) const CHROME_SELECTOR_DELTA: u8 = PALETTE_SPAN;
const INK_SLOT: u8 = FIRST_PALETTE_SLOT;

/// Extend the eight face entries with four named host colors.
///
/// The inputs use straight-alpha sRGB. IconVG receives premultiplied entries
/// in the same order, so existing face bytes keep their palette meanings.
pub fn symbol_palette(
    faces: [[u8; 4]; PALETTE_SPAN as usize],
    ink: [u8; 4],
    muted_ink: [u8; 4],
    surface: [u8; 4],
    host_seal: [u8; 4],
) -> Palette {
    let mut entries = [[0; 4]; SYMBOL_PALETTE_SPAN];
    entries[..INK_ENTRY].copy_from_slice(&faces);
    entries[INK_ENTRY..].copy_from_slice(&[ink, muted_ink, surface, host_seal]);
    palette_from_rgba(entries)
}

/// The shell activity symbols and the generic action mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    Read,
    Map,
    Inspect,
    Trail,
    Search,
    Tabs,
    Fit,
    Action,
}

impl Symbol {
    /// Every authored chrome symbol, in catalog order.
    pub const ALL: [Self; 8] = [
        Self::Read,
        Self::Map,
        Self::Inspect,
        Self::Trail,
        Self::Search,
        Self::Tabs,
        Self::Fit,
        Self::Action,
    ];

    /// The stable lowercase name of this symbol.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Map => "map",
            Self::Inspect => "inspect",
            Self::Trail => "trail",
            Self::Search => "search",
            Self::Tabs => "tabs",
            Self::Fit => "fit",
            Self::Action => "action",
        }
    }

    /// Find a symbol by its exact catalog name.
    pub fn named(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|symbol| symbol.id() == id)
    }

    /// Encode the coarse and full arms into one IconVG.
    ///
    /// # Errors
    ///
    /// Returns an error if the authored geometry cannot be encoded.
    pub fn encode(self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new(ViewBox::default());
        writer.adjust_sel(CHROME_SELECTOR_DELTA);
        writer.level_of_detail(0.0, LOD_THRESHOLD, |arm| self.draw(arm, false))?;
        writer.level_of_detail(LOD_THRESHOLD, f32::INFINITY, |arm| self.draw(arm, true))?;
        writer.adjust_sel(64 - CHROME_SELECTOR_DELTA);
        writer.finish()
    }

    fn encoded(self) -> &'static [u8] {
        static BYTES: OnceLock<[Vec<u8>; Symbol::ALL.len()]> = OnceLock::new();
        &BYTES.get_or_init(|| {
            Self::ALL.map(|symbol| symbol.encode().expect("authored symbol encodes"))
        })[self as usize]
    }

    fn draw(self, arm: &mut Writer, full: bool) -> Result<(), EncodeError> {
        let weight = if full { 5 } else { 7 };
        match self {
            Self::Read => read(arm, weight, full)?,
            Self::Map => map(arm, full)?,
            Self::Inspect => inspect(arm, weight)?,
            Self::Trail => trail(arm, weight)?,
            Self::Search => search(arm, weight)?,
            Self::Tabs => tabs(arm, weight)?,
            Self::Fit => fit(arm, weight)?,
            Self::Action => action(arm, weight as f32)?,
        }
        arm.fill_flat(INK_SLOT)
    }
}

fn polygon(arm: &mut Writer, first: (f32, f32), rest: &[(f32, f32)]) -> Result<(), EncodeError> {
    arm.move_to(first.0, first.1)?;
    arm.line_to(rest)
}

pub(crate) fn disk(
    arm: &mut Writer,
    x: f32,
    y: f32,
    radius: f32,
    reverse: bool,
) -> Result<(), EncodeError> {
    let vertical = if reverse { -radius } else { radius };
    arm.move_to(x + radius, y)?;
    arm.ellipse(4, (x, y + vertical), (x - radius, y))
}

fn ring(arm: &mut Writer, x: f32, y: f32, r: f32, weight: i32) -> Result<(), EncodeError> {
    disk(arm, x, y, r, false)?;
    disk(arm, x, y, r - weight as f32, true)
}

fn frame(arm: &mut Writer, [x0, y0, x1, y1]: [i32; 4], weight: i32) -> Result<(), EncodeError> {
    block(arm, x0, y0, x1, y1)?;
    let (l, t, r, b) = (
        (x0 + weight) as f32,
        (y0 + weight) as f32,
        (x1 - weight) as f32,
        (y1 - weight) as f32,
    );
    polygon(arm, (l, t), &[(l, b), (r, b), (r, t)])
}

fn read(arm: &mut Writer, weight: i32, full: bool) -> Result<(), EncodeError> {
    frame(arm, [-27, -22, -2, 22], weight)?;
    frame(arm, [2, -22, 27, 22], weight)?;
    if full {
        for y in [-8, 2] {
            block(arm, -18, y, -11, y + 3)?;
            block(arm, 11, y, 18, y + 3)?;
        }
    }
    Ok(())
}

fn map(arm: &mut Writer, full: bool) -> Result<(), EncodeError> {
    let radius = if full { 7.0 } else { 8.0 };
    polygon(
        arm,
        (-16.0, -9.0),
        &[(13.0, -20.0), (15.0, -14.0), (-14.0, -3.0)],
    )?;
    polygon(
        arm,
        (15.0, -15.0),
        &[(21.0, -13.0), (8.0, 22.0), (2.0, 20.0)],
    )?;
    for (x, y) in [(-20.0, -4.0), (19.0, -18.0), (4.0, 22.0)] {
        disk(arm, x, y, radius, false)?;
    }
    Ok(())
}

fn inspect(arm: &mut Writer, weight: i32) -> Result<(), EncodeError> {
    ring(arm, 0.0, 0.0, 26.0, weight)?;
    let half = (weight + 1) / 2;
    block(arm, -half, -13, half, -7)?;
    block(arm, -half, -3, half, 13)
}

fn trail(arm: &mut Writer, weight: i32) -> Result<(), EncodeError> {
    ring(arm, 0.0, 0.0, 26.0, weight)?;
    let w = weight as f32;
    polygon(
        arm,
        (-w / 2.0, -14.0),
        &[
            (w / 2.0, -14.0),
            (w / 2.0, -w / 2.0),
            (14.0, -w / 2.0),
            (14.0, w / 2.0),
            (-w / 2.0, w / 2.0),
        ],
    )
}

fn search(arm: &mut Writer, weight: i32) -> Result<(), EncodeError> {
    ring(arm, -7.0, -7.0, 19.0, weight)?;
    let d = (weight - 2) as f32;
    polygon(
        arm,
        (5.0 + d, 5.0),
        &[(26.0, 26.0 - d), (26.0 - d, 26.0), (5.0, 5.0 + d)],
    )
}

fn tabs(arm: &mut Writer, weight: i32) -> Result<(), EncodeError> {
    for y in [-25, -7, 11] {
        // At small sizes the three rows are solid so their apertures do not flicker.
        if weight == 7 {
            block(arm, -25, y + 2, 25, y + 12)?;
        } else {
            frame(arm, [-25, y, 25, y + 14], weight)?;
        }
    }
    Ok(())
}

fn fit(arm: &mut Writer, weight: i32) -> Result<(), EncodeError> {
    let w = weight as f32;
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        polygon(
            arm,
            (25.0 * x, 25.0 * y),
            &[
                (8.0 * x, 25.0 * y),
                (8.0 * x, (25.0 - w) * y),
                ((25.0 - w) * x, (25.0 - w) * y),
                ((25.0 - w) * x, 8.0 * y),
                (25.0 * x, 8.0 * y),
            ],
        )?;
    }
    Ok(())
}

fn action(arm: &mut Writer, weight: f32) -> Result<(), EncodeError> {
    polygon(
        arm,
        (-21.0, -weight / 2.0),
        &[
            (9.0, -weight / 2.0),
            (-2.0, -15.0),
            (3.0, -20.0),
            (23.0, 0.0),
            (3.0, 20.0),
            (-2.0, 15.0),
            (9.0, weight / 2.0),
            (-21.0, weight / 2.0),
        ],
    )
}
