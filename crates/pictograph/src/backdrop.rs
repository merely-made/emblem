// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The wash grammar summarizes held view facts and encodes a palette-backed IconVG.

mod digest;
mod drawing;

pub use digest::{BackdropKind, BackdropNode, LATTICE, MAX_CLUSTERS, ViewDigest};
pub use drawing::{BackdropError, LOD_HEIGHT, MAX_BYTES, MAX_PATHS, derive_backdrop};

/// The wash grammar's identity and drawing versions.
pub const WASH: crate::Grammar = crate::Grammar {
    id: "wash",
    seed_version: 1,
    drawing_version: 1,
};

/// Category entries 0–4, merged clusters at 5, activity at 6, motif at 7,
/// and transparent at 8. The host supplies the actual colors.
pub const PALETTE_ENTRIES: usize = 9;

/// The palette entry for merged clusters.
pub const OTHER: u8 = 5;
const ACTIVITY: u8 = 6;
/// The palette entry for the motif.
pub const MOTIF: u8 = 7;
/// The transparent palette entry.
pub const CLEAR: u8 = 8;
