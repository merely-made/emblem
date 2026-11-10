// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

#![cfg(feature = "pack")]

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

#[test]
fn pack_publishes_repeatable_png_sets_and_container_directories() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let run = |root: &Path| {
        Command::new(env!("CARGO_BIN_EXE_pack"))
            .arg("hocket")
            .arg(root)
            .output()
            .unwrap()
    };
    for root in [first.path(), second.path()] {
        let result = run(root);
        assert!(result.status.success(), "{result:?}");
    }
    let first_dir = first.path().join("icons/hocket");
    let second_dir = second.path().join("icons/hocket");
    let sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024];
    assert_eq!(fs::read_dir(&first_dir).unwrap().count(), sizes.len() + 3);
    for entry in fs::read_dir(&first_dir).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(
            fs::read(entry.path()).unwrap(),
            fs::read(second_dir.join(entry.file_name())).unwrap(),
            "{:?}",
            entry.file_name(),
        );
    }
    let png = |size: u16| fs::read(first_dir.join(format!("icon-{size}.png"))).unwrap();
    for size in sizes {
        let file = png(size);
        let mut decoder = png::Decoder::new(Cursor::new(file)).read_info().unwrap();
        let mut data = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut data).unwrap();
        assert_eq!(
            (info.width, info.height),
            (u32::from(size), u32::from(size))
        );
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(data[3], 0, "the frame has a transparent corner at {size}px");
        assert!(
            data.chunks_exact(4).any(|p| p == [223, 164, 74, 255]),
            "the mark paints at {size}px"
        );
    }

    let manifest_bytes = fs::read(first_dir.join("manifest.json")).unwrap();
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["app_id"], "hocket");
    assert_eq!(manifest["png_sizes"], json!(sizes));
    assert_eq!(manifest["iconvg_sha256"].as_str().unwrap().len(), 64);
    let refusal = run(first.path());
    assert!(!refusal.status.success());
    assert_eq!(
        fs::read(first_dir.join("manifest.json")).unwrap(),
        manifest_bytes
    );

    let icns = fs::read(first_dir.join("icon.icns")).unwrap();
    assert_eq!(&icns[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
        icns.len()
    );
    let mut offset = 8;
    for (kind, size) in [
        (b"icp4", 16),
        (b"ic11", 32),
        (b"icp5", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ] {
        assert_eq!(&icns[offset..offset + 4], kind);
        let length = u32::from_be_bytes(icns[offset + 4..offset + 8].try_into().unwrap()) as usize;
        assert_eq!(icns[offset + 8..offset + length], png(size));
        offset += length;
    }
    assert_eq!(offset, icns.len());

    let ico = fs::read(first_dir.join("icon.ico")).unwrap();
    assert_eq!(&ico[..6], &[0, 0, 1, 0, 7, 0]);
    let mut offset = 6 + 7 * 16;
    for (entry, size) in ico[6..offset]
        .chunks_exact(16)
        .zip([16, 24, 32, 48, 64, 128, 256])
    {
        let dim = (size % 256) as u8;
        assert_eq!(&entry[..8], &[dim, dim, 0, 0, 1, 0, 32, 0]);
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        assert_eq!(
            u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize,
            offset
        );
        assert_eq!(ico[offset..offset + length], png(size));
        offset += length;
    }
    assert_eq!(offset, ico.len());
}

#[test]
fn pack_refuses_an_unknown_id_before_creating_output() {
    let root = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_pack"))
        .arg("../other")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
