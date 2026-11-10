// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Headless packaging for authored application icons.

mod art;
mod containers;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use emblem::Palette;
use netrender::vello_backends::{CpuSession, SparseResourceLimits};
use paint_list_api::{DeviceIntSize, EngineId, LayoutPoint, LayoutRect, PaintEnvelope};
use sha2::{Digest, Sha256};

use containers::{ICNS_ENTRIES, ICO_SIZES};

/// Render an app's icon into `output_root/icons/<app-id>/`.
///
/// The destination is published after all files are written, with a manifest
/// recording the source IconVG digest, its palette and its sizes.
///
/// # Errors
///
/// Returns an error for an unknown ID, an existing destination, or a render,
/// encode, or filesystem failure.
pub fn write(app_id: &str, output_root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let app = match app_id {
        "hocket" => art::HOCKET,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown app icon {app_id:?}; available: hocket"),
            )
            .into());
        },
    };
    let parent = output_root.join("icons");
    let destination = parent.join(app.id);
    if destination.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        )
        .into());
    }

    let file = app.encode()?;
    let palette = app.palette();
    let images = render(&file, &crate::palette_from_rgba(palette))?;
    let mut source_hash = String::with_capacity(64);
    for byte in Sha256::digest(&file) {
        write!(&mut source_hash, "{byte:02x}").expect("writing to a string cannot fail");
    }
    let manifest = serde_json::json!({
        "schema": 1,
        "app_id": app.id,
        "iconvg_sha256": source_hash,
        "palette_rgba": palette,
        "png_sizes": images.keys().copied().collect::<Vec<_>>(),
        "icns_entries": ICNS_ENTRIES.map(|(kind, size)| (String::from_utf8_lossy(&kind).into_owned(), size)),
        "ico_sizes": ICO_SIZES,
    });

    fs::create_dir_all(&parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".pack-")
        .tempdir_in(&parent)?;
    for (size, png) in &images {
        fs::write(staging.path().join(format!("icon-{size}.png")), png)?;
    }
    fs::write(staging.path().join("icon.icns"), containers::icns(&images))?;
    fs::write(staging.path().join("icon.ico"), containers::ico(&images))?;
    let mut json = serde_json::to_vec_pretty(&manifest)?;
    json.push(b'\n');
    fs::write(staging.path().join("manifest.json"), json)?;
    fs::rename(staging.path(), &destination)?;
    Ok(destination)
}

fn render(file: &[u8], palette: &Palette) -> Result<BTreeMap<u16, Vec<u8>>, Box<dyn Error>> {
    let mut images = BTreeMap::new();
    let mut cpu = CpuSession::new(SparseResourceLimits::default());
    for size in ICNS_ENTRIES
        .map(|(_, size)| size)
        .into_iter()
        .chain(ICO_SIZES)
    {
        if images.contains_key(&size) {
            continue;
        }
        let side = f32::from(size);
        let target = LayoutRect::new(LayoutPoint::new(0.0, 0.0), LayoutPoint::new(side, side));
        let envelope = PaintEnvelope {
            engine: EngineId::UNASSIGNED,
            viewport: DeviceIntSize::new(i32::from(size), i32::from(size)),
            generation: 0,
            commands: crate::flat_icon::commands(file, palette, side, target)?,
            fonts: Vec::new(),
            images: Vec::new(),
        };
        let scene = paint_list_render::translate_paint_list(&envelope)?;
        let mut pixels = cpu.render(&scene)?.data_as_u8_slice().to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            if alpha != 0 && alpha != 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, u32::from(size), u32::from(size));
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixels)?;
        writer.finish()?;
        images.insert(size, png);
    }
    Ok(images)
}
