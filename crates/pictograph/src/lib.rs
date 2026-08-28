// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Name reservation for **pictograph**, the derived-face generator of the
//! Mere platform.
//!
//! A pictograph writes a picture: from a node's content address it derives a
//! compact vector face — deterministic, so every peer derives the same face
//! for the same content without shipping an asset. Faces are IconVG bytes
//! (encoded by `emblem`) with palette-indexed fills, so one blob re-themes at
//! decode time against the current seed palette, and level-of-detail rides
//! inside the face itself.
//!
//! No implementation yet. The plan is
//! `design_docs/mere_docs/implementation_strategy/2026-08-28_derived_faces_plan.md`;
//! the encoder it depends on is planned in
//! `repos/emblem/design_docs/2026-08-28_encoder_plan.md`.

#![doc(html_no_source)]
