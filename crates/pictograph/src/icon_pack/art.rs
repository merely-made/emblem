// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use emblem::ViewBox;
use emblem::encode::{EncodeError, Writer};

use crate::symbols::{CHROME_SELECTOR_DELTA, INK_ENTRY, SURFACE_ENTRY, SYMBOL_PALETTE_SPAN, disk};
use crate::text_arm::rounded_square;

pub(super) const HOCKET: App = App {
    id: "hocket",
    ink: [223, 164, 74, 255],
    surface: [24, 20, 14, 255],
    draw_mark: hocket,
};

pub(super) struct App {
    pub id: &'static str,
    ink: [u8; 4],
    surface: [u8; 4],
    draw_mark: fn(&mut Writer, bool) -> Result<(), EncodeError>,
}

impl App {
    pub fn palette(&self) -> [[u8; 4]; SYMBOL_PALETTE_SPAN] {
        let mut entries = [[0; 4]; SYMBOL_PALETTE_SPAN];
        entries[INK_ENTRY] = self.ink;
        entries[SURFACE_ENTRY] = self.surface;
        entries
    }

    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut writer = Writer::new(ViewBox::default());
        writer.adjust_sel(CHROME_SELECTOR_DELTA);
        rounded_square(&mut writer, 32.0, 17.0)?;
        writer.fill_flat(SURFACE_ENTRY as u8)?;
        writer.level_of_detail(0.0, crate::LOD_THRESHOLD, |arm| {
            (self.draw_mark)(arm, false)
        })?;
        writer.level_of_detail(crate::LOD_THRESHOLD, f32::INFINITY, |arm| {
            (self.draw_mark)(arm, true)
        })?;
        writer.adjust_sel(64 - CHROME_SELECTOR_DELTA);
        writer.finish()
    }
}

/// Three quarters of a ring, with round caps, and the answering dot.
fn hocket(arm: &mut Writer, full: bool) -> Result<(), EncodeError> {
    let center = -1.0;
    let outer = 12.0;
    let inner = if full { 8.0 } else { 7.0 };
    let dot = if full { 4.0 } else { 4.75 };
    arm.move_to(center + outer, -outer)?;
    arm.ellipse(3, (center - outer, -outer), (center - outer, outer))?;
    arm.ellipse(2, (center + outer, inner), (center + inner, inner))?;
    arm.ellipse(3, (center - inner, inner), (center - inner, -inner))?;
    arm.ellipse(2, (center + outer, -inner), (center + outer, -outer))?;
    disk(arm, 14.25, 0.0, dot, true)?;
    arm.fill_flat(INK_ENTRY as u8)
}
