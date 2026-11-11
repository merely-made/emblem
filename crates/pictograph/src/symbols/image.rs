// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use emblem::Palette;
use genet_livery::VectorImage;
use paint_list_api::{LayoutRect, PaintCmd};

use super::{Symbol, symbol_palette};

/// An authored symbol with ink supplied by the host's theme.
#[derive(Debug)]
pub struct SymbolImage {
    symbol: Symbol,
    palette: Palette,
    device_scale: f32,
}

impl SymbolImage {
    pub fn new(symbol: Symbol, ink: [u8; 4], device_scale: f32) -> Self {
        let unused = [0; 4];
        Self {
            symbol,
            palette: symbol_palette(
                [unused; crate::PALETTE_SPAN as usize],
                ink,
                unused,
                unused,
                unused,
            ),
            device_scale,
        }
    }
}

impl VectorImage for SymbolImage {
    fn intrinsic_size(&self) -> (f32, f32) {
        (16.0, 16.0)
    }

    fn paint(&self, rect: LayoutRect) -> Vec<PaintCmd> {
        let height = (rect.max.y - rect.min.y) * self.device_scale;
        crate::flat_icon::commands(self.symbol.encoded(), &self.palette, height, rect)
            .unwrap_or_default()
    }
}
