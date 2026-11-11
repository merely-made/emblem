// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use std::fmt::Write as _;

use emblem::{Call, Host, Palette, Recorder};

use super::Symbol;

impl Symbol {
    /// The silhouette as SVG path data in the default IconVG view box.
    /// The caller supplies the fill color.
    pub fn svg_path(self, height: f32) -> String {
        let file = self.encoded();
        let (_, start) = emblem::decode_metadata(file).expect("authored symbol metadata");
        let mut recorder = Recorder::default();
        emblem::execute(
            file,
            start,
            &Palette::default(),
            Host {
                height,
                ..Host::default()
            },
            &mut recorder,
        )
        .expect("authored symbol draws");
        let mut path = String::new();
        for call in recorder.calls {
            let result = match call {
                Call::MoveTo(x, y) => write!(path, "M{x} {y}"),
                Call::LineTo(x, y) => write!(path, "L{x} {y}"),
                Call::QuadTo(cx, cy, x, y) => write!(path, "Q{cx} {cy} {x} {y}"),
                Call::CubeTo(ax, ay, bx, by, x, y) => write!(path, "C{ax} {ay} {bx} {by} {x} {y}"),
                Call::Close => {
                    path.push('Z');
                    Ok(())
                },
                Call::Fill(_) => Ok(()),
            };
            result.expect("writing to a string cannot fail");
        }
        path
    }
}
