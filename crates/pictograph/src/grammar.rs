// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! What a face is: (grammar, versions, key domain, key).
//!
//! A face is fully named by four things, and a cache of faces must key on all
//! of them (iconography plan, G1 and G2):
//!
//! - the **grammar**, the family of shapes (today only [`CELLS5`]);
//! - its **seed version**, which decides *what* face a key gets — its
//!   [`Params`](crate::Params). Bumping it re-rolls every face;
//! - its **drawing version**, which decides *how* the arms draw those
//!   parameters. Bumping it changes bytes, cache keys and digest fixtures, but
//!   not any face's colours, symmetry or cells;
//! - the **key domain** and the **key**: what the key bytes mean, so the same
//!   bytes under two domains never share a face.

use emblem::encode::EncodeError;

use crate::{DERIVATION_VERSION, Params};

/// A face grammar and its two versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Grammar {
    /// A short stable name, never reused for a different grammar.
    pub id: &'static str,
    /// Mixed into the seed: bumping it changes every face's parameters.
    pub seed_version: u32,
    /// Not in the seed: bumping it changes only how the parameters are drawn.
    pub drawing_version: u32,
}

/// The 5×5 cell grammar: three symmetries, block or diamond, two colours from
/// eight palette entries, a coarse 3×3 arm below
/// [`LOD_THRESHOLD`](crate::LOD_THRESHOLD).
pub const CELLS5: Grammar = Grammar {
    id: "cells5",
    seed_version: DERIVATION_VERSION,
    drawing_version: 1,
};

/// What a face's key bytes mean.
///
/// Two domains given the same key bytes derive different faces, so a key
/// reused across contexts does not link them by its face.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyDomain {
    /// A canonical node address: the canvas's `node.url()`, a content address,
    /// a release or series address. These are public already, so a face from
    /// one adds no linkability. [`derive`](crate::derive) uses this domain, and
    /// its seed is the original, unseparated one, so no existing face moves.
    Address,
    /// A key that must not be linkable across contexts, such as a persona key:
    /// the face derives from (context, key), so the same key in two contexts
    /// shows two unrelated faces.
    Context(String),
}

/// Everything that names one face: the cache key for derived bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FaceKey {
    /// The grammar, with both versions.
    pub grammar: Grammar,
    /// What the key means.
    pub domain: KeyDomain,
    /// The key bytes.
    pub key: Vec<u8>,
}

impl FaceKey {
    /// The [`CELLS5`] face of a canonical address: what the canvas draws.
    pub fn address(address: impl Into<Vec<u8>>) -> Self {
        Self {
            grammar: CELLS5,
            domain: KeyDomain::Address,
            key: address.into(),
        }
    }

    /// The face's IconVG bytes.
    ///
    /// # Errors
    ///
    /// Whatever the encoder refuses; [`CELLS5`] faces always encode.
    pub fn derive(&self) -> Result<Vec<u8>, EncodeError> {
        // CELLS5 is the only grammar; a second one dispatches on `grammar.id`.
        crate::encode(&self.params())
    }

    /// The face's parameters.
    pub fn params(&self) -> Params {
        crate::params_from_seed(seed(&self.domain, &self.key))
    }
}

/// The [`CELLS5`] parameters of `key` read under `domain`.
pub fn params_in(domain: &KeyDomain, key: &[u8]) -> Params {
    crate::params_from_seed(seed(domain, key))
}

/// The [`CELLS5`] face of `key` read under `domain`, as IconVG bytes.
///
/// # Errors
///
/// Whatever the encoder refuses; [`CELLS5`] faces always encode.
pub fn derive_in(domain: &KeyDomain, key: &[u8]) -> Result<Vec<u8>, EncodeError> {
    crate::encode(&params_in(domain, key))
}

/// FNV-1a 64 over `bytes`, from the standard offset basis.
fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A tag separating context-domain seeds from address seeds.
const CONTEXT_TAG: u64 = 0x6374_7874_2d6b_6579; // "ctxt-key"

/// The 64-bit seed of a key, with the seed version folded in.
///
/// [`KeyDomain::Address`] keeps the original seed exactly: FNV-1a over the
/// address. A context domain hashes the context's length, the context and the
/// key, so `("ab", "c")` and `("a", "bc")` differ, then folds in a tag so no
/// context seed is an address seed by construction of the input alone.
///
/// FNV-1a is small, exactly specified and identical on every machine, which
/// is the whole requirement. It is not cryptographic, and faces are not proof
/// of anything (see the crate docs).
pub(crate) fn seed(domain: &KeyDomain, key: &[u8]) -> u64 {
    let hash = match domain {
        KeyDomain::Address => fnv1a(key.iter().copied()),
        KeyDomain::Context(context) => {
            let length = (context.len() as u64).to_le_bytes();
            let bytes = length
                .into_iter()
                .chain(context.bytes())
                .chain(key.iter().copied());
            fnv1a(bytes) ^ CONTEXT_TAG
        },
    };
    hash ^ u64::from(CELLS5.seed_version).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}
