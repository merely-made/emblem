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
//! **Scale-aware.** Each face carries two arms behind a level-of-detail
//! branch: a single bold silhouette when drawn small, the full figure when
//! drawn large. The decoder picks; the caller does nothing.
//!
//! # Versioning
//!
//! [`DERIVATION_VERSION`] is mixed into the seed, so bumping it changes every
//! face everywhere. That is a deliberate, visible act — every node in every
//! session changes appearance — and never a side effect of tidying this code.
//! Changing the grammar, the parameter mapping, or the geometry without
//! bumping it is the actual mistake, because two peers on different versions
//! would then derive different bytes and silently disagree.

#![doc(html_no_source)]

use emblem::ViewBox;
use emblem::encode::{EncodeError, Writer};

#[cfg(feature = "vello")]
pub mod vello;

/// Bumping this changes every derived face. See the module docs.
pub const DERIVATION_VERSION: u32 = 2;

/// Cells across and down.
pub const GRID: usize = 5;

/// A cell's side, in graphic units.
const CELL: i32 = 12;

/// The grid's top-left corner. `GRID * CELL` is 60, centred in the default
/// ViewBox's 64 units, which leaves a two-unit margin and — the reason for
/// these particular numbers — keeps every coordinate an integer in
/// `[-30, 30]`, so all of them encode in one byte.
const ORIGIN: i32 = -30;

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

/// A 64-bit hash of the address, with the derivation version folded in.
///
/// FNV-1a: small, exactly specified, and identical on every machine, which is
/// the whole requirement. It is not, and need not be, cryptographic — the
/// input is already a content address.
fn seed_of(address: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in address {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^ u64::from(DERIVATION_VERSION).wrapping_mul(0x9E37_79B9_7F4A_7C15)
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

/// Resolve a content address to its parameters.
pub fn params_of(address: &[u8]) -> Params {
    let mut stream = Stream(seed_of(address));

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
                }
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

/// Derive a face from a content address, as IconVG bytes.
///
/// The same address always yields the same bytes. See the module docs for what
/// that buys and what it costs.
pub fn derive(address: &[u8]) -> Result<Vec<u8>, EncodeError> {
    encode(&params_of(address))
}

/// Encode a face from parameters, for callers holding them already.
pub fn encode(params: &Params) -> Result<Vec<u8>, EncodeError> {
    let mut w = Writer::new(ViewBox::default());

    // The small arm: one bold shape covering everything that is filled, in the
    // primary colour. At a few pixels across, detail is noise.
    let bounds = filled_bounds(params);
    w.level_of_detail(0.0, LOD_THRESHOLD, |arm| {
        block(arm, bounds.0, bounds.1, bounds.2, bounds.3)?;
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

/// The bounding box of the filled cells, in graphic units.
fn filled_bounds(params: &Params) -> (i32, i32, i32, i32) {
    let (mut min_col, mut min_row) = (GRID, GRID);
    let (mut max_col, mut max_row) = (0usize, 0usize);
    for row in 0..GRID {
        for col in 0..GRID {
            if params.cells[row][col] == Cell::Empty {
                continue;
            }
            min_col = min_col.min(col);
            max_col = max_col.max(col);
            min_row = min_row.min(row);
            max_row = max_row.max(row);
        }
    }
    // `params_of` guarantees at least one filled cell.
    debug_assert!(min_col < GRID, "a face must have at least one filled cell");
    (
        ORIGIN + (min_col as i32) * CELL,
        ORIGIN + (min_row as i32) * CELL,
        ORIGIN + (max_col as i32 + 1) * CELL,
        ORIGIN + (max_row as i32 + 1) * CELL,
    )
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
mod tests {
    use super::*;
    use emblem::{Call, Host, Paint, Palette, Recorder, Rgba, decode_metadata, execute};

    fn addresses() -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = (0..64u8).map(|i| vec![i]).collect();
        out.push(b"".to_vec());
        out.push(b"hello".to_vec());
        out.push(b"a rather longer content address, 32 bytes+".to_vec());
        out.push(vec![0xFF; 64]);
        out
    }

    fn run(file: &[u8], palette: &Palette, height: f32) -> Vec<Call> {
        let (_, start) = decode_metadata(file).expect("metadata");
        let mut sink = Recorder::default();
        execute(
            file,
            start,
            palette,
            Host {
                features: 0,
                height,
            },
            &mut sink,
        )
        .expect("execution");
        sink.calls
    }

    fn palette_of(colors: &[(usize, Rgba)]) -> Palette {
        let mut entries = Palette::default().0;
        for (index, color) in colors {
            entries[*index] = *color;
        }
        Palette::new(entries).unwrap()
    }

    fn fills(calls: &[Call]) -> Vec<Rgba> {
        calls
            .iter()
            .filter_map(|c| match c {
                Call::Fill(Paint::Flat(color)) => Some(*color),
                _ => None,
            })
            .collect()
    }

    // ---- determinism --------------------------------------------------------

    #[test]
    fn the_same_address_always_gives_the_same_bytes() {
        for address in addresses() {
            let once = derive(&address).unwrap();
            for _ in 0..8 {
                assert_eq!(derive(&address).unwrap(), once, "address {address:?}");
            }
        }
    }

    /// A digest of a face, for pinning it without a 200-byte literal.
    ///
    /// Deliberately the same FNV-1a the derivation seeds from: one exactly
    /// specified function, reused, rather than a second one to keep correct.
    fn digest(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    /// The digest-pinned fixtures: committed values pinning the derivation.
    ///
    /// These literals are the point. A test that recomputed both sides would
    /// pass no matter how the grammar changed, which is no test at all. When
    /// this fails, the mapping has moved and **every peer's faces have
    /// changed** — so the response is to bump [`DERIVATION_VERSION`]
    /// deliberately and then update these, never to quietly update these.
    #[test]
    fn the_derivation_matches_its_digest_pinned_fixtures() {
        // (address, byte length, digest, filled cells)
        const GOLDEN: [(&[u8], usize, u64, usize); 4] = [
            (b"", 51, 0x688D_4036_1B63_DB24, 3),
            (b"a", 147, 0xDF7E_ECEC_A190_53D7, 15),
            (b"pictograph", 171, 0x04D6_6F6B_690B_F810, 18),
            (b"\x00\x01\x02\x03", 187, 0xF844_3002_6212_9CAA, 20),
        ];
        for (address, len, want, filled) in GOLDEN {
            let face = derive(address).unwrap();
            assert_eq!(face.len(), len, "length for {address:?}");
            assert_eq!(digest(&face), want, "digest for {address:?}");
            assert_eq!(
                params_of(address).filled_count(),
                filled,
                "cells for {address:?}"
            );
        }
    }

    #[test]
    fn the_grammar_does_not_collapse_to_one_variant() {
        // A mapping bug that always picked the same symmetry or form would
        // still pass every other test here while making every face alike.
        let mut symmetries = std::collections::HashSet::new();
        let mut forms = std::collections::HashSet::new();
        let mut primaries = std::collections::HashSet::new();
        for address in addresses() {
            let p = params_of(&address);
            symmetries.insert(format!("{:?}", p.symmetry));
            forms.insert(format!("{:?}", p.form));
            primaries.insert(p.primary);
        }
        assert_eq!(symmetries.len(), 3, "all three symmetries must occur");
        assert_eq!(forms.len(), 2, "both forms must occur");
        assert!(
            primaries.len() >= 6,
            "only {} distinct primary colours across the address set",
            primaries.len(),
        );
    }

    #[test]
    fn different_addresses_generally_give_different_faces() {
        let mut seen = std::collections::HashSet::new();
        for address in addresses() {
            seen.insert(derive(&address).unwrap());
        }
        // Collisions are possible in principle; a near-total spread is the
        // property worth asserting.
        assert!(
            seen.len() >= addresses().len() * 3 / 4,
            "only {} distinct faces from {} addresses",
            seen.len(),
            addresses().len(),
        );
    }

    // ---- theming ------------------------------------------------------------

    #[test]
    fn one_face_wears_two_palettes() {
        // The property the whole design rests on: identical bytes, different
        // palettes, different colours — with no re-derivation.
        let face = derive(b"theme me").unwrap();
        let params = params_of(b"theme me");

        let red = Rgba::new(0xFF, 0x00, 0x00, 0xFF);
        let blue = Rgba::new(0x00, 0x00, 0xFF, 0xFF);
        let light = palette_of(&[(usize::from(params.primary), red)]);
        let dark = palette_of(&[(usize::from(params.primary), blue)]);

        assert!(fills(&run(&face, &light, 8.0)).contains(&red));
        assert!(fills(&run(&face, &dark, 8.0)).contains(&blue));
    }

    #[test]
    fn every_fill_resolves_to_a_palette_entry_never_a_literal() {
        // A literal colour would render identically under every palette, which
        // is exactly the bug this crate must not have. Give the palette
        // distinctive colours and check nothing else appears.
        let markers: Vec<Rgba> = (0..PALETTE_SPAN)
            .map(|i| Rgba::new(i * 16 + 1, 0x00, 0x00, 0xFF))
            .collect();
        let palette = palette_of(
            &markers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, *c))
                .collect::<Vec<_>>(),
        );

        for address in addresses() {
            let face = derive(&address).unwrap();
            for height in [8.0, 64.0] {
                for color in fills(&run(&face, &palette, height)) {
                    assert!(
                        markers.contains(&color),
                        "address {address:?} at height {height} filled with {color:?}, \
                         which is not a palette entry",
                    );
                }
            }
        }
    }

    // ---- level of detail ----------------------------------------------------

    #[test]
    fn each_face_draws_a_simpler_arm_when_small() {
        let mut simpler = 0;
        for address in addresses() {
            let face = derive(&address).unwrap();
            let small = run(&face, &Palette::default(), 8.0);
            let large = run(&face, &Palette::default(), 128.0);

            // The small arm is exactly one shape and one fill, always.
            assert_eq!(fills(&small).len(), 1, "address {address:?}");
            assert!(!fills(&large).is_empty(), "address {address:?}");

            let ops = |calls: &[Call]| calls.len();
            if ops(&large) > ops(&small) {
                simpler += 1;
            }
        }
        // A single-cell face can tie; most must genuinely simplify.
        assert!(
            simpler >= addresses().len() * 3 / 4,
            "only {simpler} of {} faces simplified when small",
            addresses().len(),
        );
    }

    #[test]
    fn the_arms_switch_at_the_threshold() {
        let face = derive(b"threshold").unwrap();
        let below = run(&face, &Palette::default(), LOD_THRESHOLD - 0.1);
        let at = run(&face, &Palette::default(), LOD_THRESHOLD);
        assert_ne!(below, at, "the arm must change at the threshold");
        assert_eq!(fills(&below).len(), 1, "below is the single bold shape");
    }

    // ---- structure ----------------------------------------------------------

    #[test]
    fn a_face_is_never_blank() {
        for address in addresses() {
            assert!(
                params_of(&address).filled_count() > 0,
                "address {address:?}"
            );
        }
    }

    #[test]
    fn symmetry_holds_for_every_address() {
        for address in addresses() {
            let p = params_of(&address);
            for row in 0..GRID {
                for col in 0..GRID {
                    let (mr, mc) = (GRID - 1 - row, GRID - 1 - col);
                    let here = p.cells[row][col];
                    match p.symmetry {
                        Symmetry::MirrorX => {
                            assert_eq!(here, p.cells[row][mc], "mirror-X at ({row},{col})");
                        }
                        Symmetry::MirrorBoth => {
                            assert_eq!(
                                here, p.cells[row][mc],
                                "mirror-both horizontal at ({row},{col})"
                            );
                            assert_eq!(
                                here, p.cells[mr][col],
                                "mirror-both vertical at ({row},{col})"
                            );
                        }
                        Symmetry::Rotate180 => {
                            assert_eq!(here, p.cells[mr][mc], "rotate-180 at ({row},{col})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn independent_regions_sample_each_orbit_once() {
        for (symmetry, expected) in [
            (Symmetry::MirrorX, 15),
            (Symmetry::MirrorBoth, 9),
            (Symmetry::Rotate180, 13),
        ] {
            let sampled = (0..GRID)
                .flat_map(|row| (0..GRID).map(move |col| (row, col)))
                .filter(|&(row, col)| is_independent_cell(symmetry, row, col))
                .count();
            assert_eq!(sampled, expected, "{symmetry:?}");

            for row in 0..GRID {
                for col in 0..GRID {
                    let (mr, mc) = (GRID - 1 - row, GRID - 1 - col);
                    let orbit: std::collections::HashSet<_> = match symmetry {
                        Symmetry::MirrorX => [(row, col), (row, mc)].into_iter().collect(),
                        Symmetry::MirrorBoth => [(row, col), (row, mc), (mr, col), (mr, mc)]
                            .into_iter()
                            .collect(),
                        Symmetry::Rotate180 => [(row, col), (mr, mc)].into_iter().collect(),
                    };
                    let representatives = orbit
                        .into_iter()
                        .filter(|&(orbit_row, orbit_col)| {
                            is_independent_cell(symmetry, orbit_row, orbit_col)
                        })
                        .count();
                    assert_eq!(
                        representatives, 1,
                        "{symmetry:?} orbit containing ({row},{col})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_colours_are_always_distinct() {
        for address in addresses() {
            let p = params_of(&address);
            assert_ne!(p.primary, p.accent, "address {address:?}");
            assert!(p.primary < PALETTE_SPAN && p.accent < PALETTE_SPAN);
        }
    }

    #[test]
    fn faces_stay_small() {
        for address in addresses() {
            let face = derive(&address).unwrap();
            assert!(
                face.len() < 512,
                "address {address:?} produced {} bytes",
                face.len(),
            );
        }
    }

    // ---- fuzz ---------------------------------------------------------------

    #[test]
    fn random_addresses_produce_decodable_faces() {
        let mut stream = Stream(0x1234_5678_9ABC_DEF0);
        for case in 0..300 {
            let len = (stream.below(48) + 1) as usize;
            let address: Vec<u8> = (0..len).map(|_| (stream.below(256)) as u8).collect();
            let face = derive(&address).unwrap();
            // Decoding must not panic or error at either extreme.
            for height in [0.0, 1.0, LOD_THRESHOLD, 4096.0] {
                let calls = run(&face, &Palette::default(), height);
                assert!(!calls.is_empty(), "case {case} at height {height}");
            }
        }
    }
}
