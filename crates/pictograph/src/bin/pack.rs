// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(app_id), Some(output_root), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: pack <app-id> <output-root>");
        return ExitCode::FAILURE;
    };
    let Some(app_id) = app_id.to_str() else {
        eprintln!("pack: app id must be UTF-8");
        return ExitCode::FAILURE;
    };
    match pictograph::icon_pack::write(app_id, &PathBuf::from(output_root)) {
        Ok(destination) => {
            println!("{}", destination.display());
            ExitCode::SUCCESS
        },
        Err(error) => {
            eprintln!("pack: {error}");
            ExitCode::FAILURE
        },
    }
}
