// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;

pub(super) const ICNS_ENTRIES: [([u8; 4], u16); 10] = [
    (*b"icp4", 16),
    (*b"ic11", 32),
    (*b"icp5", 32),
    (*b"ic12", 64),
    (*b"ic07", 128),
    (*b"ic13", 256),
    (*b"ic08", 256),
    (*b"ic14", 512),
    (*b"ic09", 512),
    (*b"ic10", 1024),
];

pub(super) const ICO_SIZES: [u16; 7] = [16, 24, 32, 48, 64, 128, 256];

pub(super) fn icns(images: &BTreeMap<u16, Vec<u8>>) -> Vec<u8> {
    let mut out = b"icns".to_vec();
    let length = 8 + ICNS_ENTRIES
        .iter()
        .map(|(_, size)| 8 + images[size].len())
        .sum::<usize>();
    out.extend((length as u32).to_be_bytes());
    for (kind, size) in ICNS_ENTRIES {
        let png = &images[&size];
        out.extend(kind);
        out.extend(((8 + png.len()) as u32).to_be_bytes());
        out.extend(png);
    }
    out
}

pub(super) fn ico(images: &BTreeMap<u16, Vec<u8>>) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend((ICO_SIZES.len() as u16).to_le_bytes());
    let mut offset = (out.len() + 16 * ICO_SIZES.len()) as u32;
    for size in ICO_SIZES {
        let dim = if size == 256 { 0 } else { size as u8 };
        // Width, height, palette count, reserved, planes, bits per pixel.
        out.extend([dim, dim, 0, 0, 1, 0, 32, 0]);
        let length = images[&size].len() as u32;
        out.extend(length.to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += length;
    }
    for size in ICO_SIZES {
        out.extend(&images[&size]);
    }
    out
}
