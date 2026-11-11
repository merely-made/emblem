// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! **pictograph** derives node faces: a content address in, a compact vector
//! face out.
//!
//! A pictograph writes a picture. From a node's content address this derives a
//! small symmetric mark, encoded as IconVG bytes by
//! [`emblem`](https://crates.io/crates/emblem). Every peer derives the same
//! face for the same content, so a face never has to be shipped — and when one
//! is shipped it is a couple of hundred bytes.
//!
//! # Three properties, and how each is obtained
//!
//! **Deterministic.** The same address always gives the same bytes, on every
//! machine and in every process. The derivation reads its entropy from a
//! fixed hash of the address, uses only integer arithmetic, and encodes
//! through emblem, which guarantees the same at its layer. Faces can therefore
//! be content-addressed themselves, and two peers agree without talking.
//!
//! **Themeable.** Fills name *palette slots*, never literal colours. IconVG
//! pre-loads its registers from the palette the decoder's caller supplies, and
//! at the starting `SEL` of 56 the slot `8 + k` addresses register `k`, which
//! holds custom palette entry `k`. So a face is themed by re-decoding it
//! against a different palette — no re-derivation, no stored variants, and no
//! register ops in the file at all. A literal colour anywhere here would be a
//! bug, not a shortcut.
//!
//! **Scale-aware.** Each face carries three abutting arms behind
//! level-of-detail branches: below [`TEXT_THRESHOLD`] a single mark for text,
//! tabs and rows (see `text_arm`); up to [`LOD_THRESHOLD`] a coarse 3x3
//! silhouette; above it the full 5x5 figure. At Canvas's normal 25.92px face
//! height, every coarse cell is 8.1px wide. The decoder picks by height; the
//! caller does nothing.
//!
//! # What names a face
//!
//! A face is (grammar, versions, key domain, key) — a [`FaceKey`], which is
//! also the cache key for its bytes. The grammar here is [`CELLS5`].
//!
//! **Two versions.** The seed version, [`DERIVATION_VERSION`], is mixed into
//! the seed, so bumping it changes every face's colours, symmetry and cells.
//! That is a deliberate, visible act — every node in every session changes
//! appearance — and never a side effect of tidying this code. The drawing
//! version, [`Grammar::drawing_version`], covers how the arms draw those
//! parameters: bumping it changes bytes and cache keys but no face's identity.
//! Changing the parameter mapping without bumping the seed version, or the
//! geometry without bumping the drawing version, is the actual mistake,
//! because two peers would then derive different bytes and silently disagree.
//!
//! **Key domains.** [`KeyDomain::Address`] is a canonical node address, and
//! [`derive`] uses it. [`KeyDomain::Context`] separates keys that must not link
//! across contexts: the same key bytes under two domains give unrelated faces.
//!
//! # What a face is not
//!
//! **A face is a linkability handle.** A stable face derived from a key lets
//! anyone who sees it correlate every appearance of that key. Content,
//! release and series addresses are public already, so faces from them add
//! nothing. A persona key is different: an unassociated or anonymous persona
//! gets no derived face, or one derived under a [`KeyDomain::Context`], so two
//! contexts do not match; a face is never derived from a publisher key the
//! publisher has not associated. Each consumer states which key its faces
//! derive from.
//!
//! **A face is a recognition aid, not proof.** The seed is a 64-bit FNV-1a
//! hash, which is not cryptographic, and the visible distinctions are far
//! fewer (eight primaries, seven accents, three symmetries, two forms, a
//! sparse grid), so a look-alike for any face can be ground out in moments. No
//! surface treats matching faces as evidence of matching identity. Comparing
//! keys between peers by eye would need its own grammar with a stated bit
//! budget and fixed contrast.

#![doc(html_no_source)]

pub mod backdrop;
pub mod blazon;
pub mod symbols;

#[cfg(feature = "pack")]
pub mod icon_pack;

#[cfg(feature = "paint-list")]
pub mod flat_icon;

mod grammar;
mod text_arm;

pub use grammar::{CELLS5, FaceKey, Grammar, KeyDomain, derive_in, params_in};

use emblem::{Palette, Rgba, ViewBox};
use emblem::encode::{EncodeError, Writer};

#[cfg(feature = "vello")]
pub mod vello;

/// The [`CELLS5`] seed version. Bumping it changes every derived face. See the
/// module docs.
pub const DERIVATION_VERSION: u32 = 3;

/// Cells across and down.
pub const GRID: usize = 5;

/// A cell's side, in graphic units.
const CELL: i32 = 12;

/// Cells across and down in the deliberately coarse small arm.
const SMALL_GRID: usize = 3;

/// A small-arm cell's side, in graphic units. At Canvas's normal 25.92px
/// face height this is 8.1 physical pixels, large enough to remain a shape
/// rather than anti-aliased texture.
const SMALL_CELL: i32 = 20;

/// The grid's top-left corner. `GRID * CELL` is 60, centred in the default
/// ViewBox's 64 units, which leaves a two-unit margin and — the reason for
/// these particular numbers — keeps every coordinate an integer in
/// `[-30, 30]`, so all of them encode in one byte.
const ORIGIN: i32 = -30;

/// The rasterization height at which the coarse arm takes over from the text
/// arm. Fourteen keeps every coarse cell at 4.4 px or more, and is a one-byte
/// coordinate.
pub const TEXT_THRESHOLD: f32 = 14.0;

/// The rasterization height at which the detailed arm takes over.
pub const LOD_THRESHOLD: f32 = 32.0;

/// How many custom palette entries the derivation draws colours from.
pub const PALETTE_SPAN: u8 = 8;

/// The slot that addresses custom palette entry 0 at the starting `SEL`.
const FIRST_PALETTE_SLOT: u8 = 8;

/// How a face's left half determines the rest of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symmetry {
    /// Mirrored left to right.
    MirrorX,
    /// Mirrored both ways, so a quarter determines the whole.
    MirrorBoth,
    /// Rotated a half turn about the centre.
    Rotate180,
}

/// The shape a filled cell takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// A filled square.
    Block,
    /// A filled diamond, its points at the cell's edge midpoints.
    Diamond,
}

/// What a cell holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Empty,
    Primary,
    Accent,
}

/// The parameters a content address resolves to.
///
/// Exposed because a caller that wants to explain a face — or test one —
/// should not have to read the bytes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// The symmetry the grid was filled under.
    pub symmetry: Symmetry,
    /// The shape each filled cell takes.
    pub form: Form,
    /// Custom palette entry for the dominant colour.
    pub primary: u8,
    /// Custom palette entry for the secondary colour.
    pub accent: u8,
    cells: [[Cell; GRID]; GRID],
}

impl Params {
    /// Whether cell `(col, row)` is filled at all.
    pub fn is_filled(&self, col: usize, row: usize) -> bool {
        self.cells[row][col] != Cell::Empty
    }

    /// How many cells are filled.
    pub fn filled_count(&self) -> usize {
        self.cells
            .iter()
            .flatten()
            .filter(|c| **c != Cell::Empty)
            .count()
    }
}

/// SplitMix64: a fixed, well-specified stream from the seed.
struct Stream(u64);

impl Stream {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Whether a cell is the row-major representative of its symmetry orbit.
///
/// Sampling exactly one representative matters because each sample advances
/// the seeded stream. Sampling a mirrored cell again would overwrite the first
/// value and make the parameter mapping depend on redundant draws.
fn is_independent_cell(symmetry: Symmetry, row: usize, col: usize) -> bool {
    let (mirror_row, mirror_col) = (GRID - 1 - row, GRID - 1 - col);
    match symmetry {
        Symmetry::MirrorX => col <= mirror_col,
        Symmetry::MirrorBoth => row <= mirror_row && col <= mirror_col,
        Symmetry::Rotate180 => (row, col) <= (mirror_row, mirror_col),
    }
}

/// Resolve a canonical address to its parameters.
pub fn params_of(address: &[u8]) -> Params {
    grammar::params_in(&KeyDomain::Address, address)
}

/// The parameters a seed resolves to.
fn params_from_seed(seed: u64) -> Params {
    let mut stream = Stream(seed);
    let symmetry = match stream.below(3) {
        0 => Symmetry::MirrorX,
        1 => Symmetry::MirrorBoth,
        _ => Symmetry::Rotate180,
    };
    let form = if stream.below(2) == 0 {
        Form::Block
    } else {
        Form::Diamond
    };

    let primary = (stream.below(u64::from(PALETTE_SPAN))) as u8;
    // Distinct from the primary, chosen among the remaining entries so the two
    // never collide and a face always reads as two-toned.
    let accent = {
        let offset = 1 + stream.below(u64::from(PALETTE_SPAN) - 1) as u8;
        (primary + offset) % PALETTE_SPAN
    };

    // Sparse, medium or dense, as a chance in sixteenths that a cell is filled.
    let fill_chance = match stream.below(3) {
        0 => 5,
        1 => 8,
        _ => 11,
    };
    // How often a filled cell takes the accent rather than the primary.
    let accent_chance = 4 + stream.below(4);

    let mut cells = [[Cell::Empty; GRID]; GRID];
    // Only the independent region is drawn from the stream; symmetry supplies
    // the rest, which is what makes a face read as designed rather than noisy.
    for row in 0..GRID {
        for col in 0..GRID {
            if !is_independent_cell(symmetry, row, col) {
                continue;
            }
            let cell = if stream.below(16) < fill_chance {
                if stream.below(16) < accent_chance {
                    Cell::Accent
                } else {
                    Cell::Primary
                }
            } else {
                Cell::Empty
            };
            cells[row][col] = cell;
            let (mirror_col, mirror_row) = (GRID - 1 - col, GRID - 1 - row);
            match symmetry {
                Symmetry::MirrorX => cells[row][mirror_col] = cell,
                Symmetry::MirrorBoth => {
                    cells[row][mirror_col] = cell;
                    cells[mirror_row][col] = cell;
                    cells[mirror_row][mirror_col] = cell;
                },
                Symmetry::Rotate180 => cells[mirror_row][mirror_col] = cell,
            }
        }
    }

    // A face is never blank: an empty grid would also be an empty
    // level-of-detail arm, which the encoder refuses outright.
    if cells.iter().flatten().all(|c| *c == Cell::Empty) {
        cells[GRID / 2][GRID / 2] = Cell::Primary;
    }

    Params {
        symmetry,
        form,
        primary,
        accent,
        cells,
    }
}

/// Derive the [`CELLS5`] face of a canonical address, as IconVG bytes.
///
/// The same address always yields the same bytes. See the module docs for what
/// that buys and what it costs.
///
/// # Errors
///
/// Whatever the encoder refuses; every [`CELLS5`] face encodes.
pub fn derive(address: &[u8]) -> Result<Vec<u8>, EncodeError> {
    encode(&params_of(address))
}

/// Encode a face from parameters, for callers holding them already.
///
/// # Errors
///
/// Whatever the encoder refuses; every [`CELLS5`] face encodes.
pub fn encode(params: &Params) -> Result<Vec<u8>, EncodeError> {
    let mut w = Writer::new(ViewBox::default());

    // The small arm is a 3x3 majority reduction of the detailed 5x5 grid. It
    // retains the face's coarse geometry but makes every feature 20 graphic
    // units wide (8.1px at Canvas's regular 25.92px face height). The old
    // bounding rectangle was bold but lost almost every address-specific
    // distinction at that size.
    w.level_of_detail(0.0, TEXT_THRESHOLD, |arm| text_arm::draw(arm, params))?;

    let small = small_cells(params);
    w.level_of_detail(TEXT_THRESHOLD, LOD_THRESHOLD, |arm| {
        for (row, cells) in small.iter().enumerate() {
            for (col, filled) in cells.iter().enumerate() {
                if !filled {
                    continue;
                }
                let (x0, y0) = (
                    ORIGIN + (col as i32) * SMALL_CELL,
                    ORIGIN + (row as i32) * SMALL_CELL,
                );
                block(arm, x0, y0, x0 + SMALL_CELL, y0 + SMALL_CELL)?;
            }
        }
        arm.fill_flat(slot_for(params.primary))
    })?;

    // The large arm: every filled cell, primary group then accent group, each
    // group drawn as one path and filled once.
    w.level_of_detail(LOD_THRESHOLD, f32::INFINITY, |arm| {
        for (group, palette_entry) in [
            (Cell::Primary, params.primary),
            (Cell::Accent, params.accent),
        ] {
            let mut drew = false;
            for row in 0..GRID {
                for col in 0..GRID {
                    if params.cells[row][col] != group {
                        continue;
                    }
                    let (x0, y0) = (ORIGIN + (col as i32) * CELL, ORIGIN + (row as i32) * CELL);
                    let (x1, y1) = (x0 + CELL, y0 + CELL);
                    match params.form {
                        Form::Block => block(arm, x0, y0, x1, y1)?,
                        Form::Diamond => diamond(arm, x0, y0, x1, y1)?,
                    }
                    drew = true;
                }
            }
            if drew {
                arm.fill_flat(slot_for(palette_entry))?;
            }
        }
        Ok(())
    })?;

    w.finish()
}

/// The slot that addresses custom palette entry `entry`.
///
/// At the starting `SEL` of 56, slot `8 + k` lands on register `k`, which
/// IconVG pre-loads from the custom palette. Naming a slot is therefore
/// naming a palette entry, with no register op in the file.
fn slot_for(entry: u8) -> u8 {
    FIRST_PALETTE_SLOT + (entry % PALETTE_SPAN)
}

/// Lower the caller's straight-alpha colors to IconVG's premultiplied palette.
///
/// Entries past `N` keep the default palette's values.
pub fn palette_from_rgba<const N: usize>(entries: [[u8; 4]; N]) -> Palette {
    let mut palette = Palette::default();
    for (target, [r, g, b, a]) in palette.0[..N].iter_mut().zip(entries) {
        let premultiply = |channel: u8| ((u32::from(channel) * u32::from(a) + 127) / 255) as u8;
        *target = Rgba::new(premultiply(r), premultiply(g), premultiply(b), a);
    }
    palette
}

/// Reduce the detailed 5x5 grid to its small-arm 3x3 silhouette.
///
/// The middle row and column remain literal centre cells; the four outer
/// rows/columns become paired bands. A coarse cell is filled when at least
/// half of its source cells are filled. This makes an outer 2x2 region require
/// two detailed cells, avoiding the near-solid silhouettes that an "any cell"
/// reduction creates on dense faces. Empty reductions resolve to the centre,
/// keeping the IconVG arm non-empty without breaking any supported symmetry.
fn small_cells(params: &Params) -> [[bool; SMALL_GRID]; SMALL_GRID] {
    let mut small = [[false; SMALL_GRID]; SMALL_GRID];
    for (small_row, row) in small.iter_mut().enumerate() {
        let (source_row_start, source_row_end) = small_source_span(small_row);
        for (small_col, filled) in row.iter_mut().enumerate() {
            let (source_col_start, source_col_end) = small_source_span(small_col);
            let total = (source_row_end - source_row_start) * (source_col_end - source_col_start);
            let source_filled = (source_row_start..source_row_end)
                .flat_map(|row| {
                    (source_col_start..source_col_end).map(move |col| params.is_filled(col, row))
                })
                .filter(|filled| *filled)
                .count();
            *filled = source_filled * 2 >= total;
        }
    }
    if small.iter().flatten().all(|filled| !filled) {
        small[SMALL_GRID / 2][SMALL_GRID / 2] = true;
    }
    small
}

/// Map a coarse grid axis to a centred 5x5 source band.
fn small_source_span(index: usize) -> (usize, usize) {
    match index {
        0 => (0, 2),
        1 => (2, 3),
        2 => (3, 5),
        _ => unreachable!("small grid index must be in range"),
    }
}

/// An axis-aligned rectangle, as one parallelogram op.
fn block(w: &mut Writer, x0: i32, y0: i32, x1: i32, y1: i32) -> Result<(), EncodeError> {
    w.move_to(x0 as f32, y0 as f32)?;
    // The fourth corner, A - B + C, comes out at (x0, y1): the rectangle.
    w.parallelogram((x1 as f32, y0 as f32), (x1 as f32, y1 as f32))
}

/// A diamond inscribed in the cell, as one parallelogram op.
fn diamond(w: &mut Writer, x0: i32, y0: i32, x1: i32, y1: i32) -> Result<(), EncodeError> {
    let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
    w.move_to(cx as f32, y0 as f32)?;
    // A - B + C lands on (x0, cy), the fourth point of the diamond.
    w.parallelogram((x1 as f32, cy as f32), (cx as f32, y1 as f32))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
