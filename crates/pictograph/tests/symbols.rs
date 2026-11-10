// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use std::collections::HashSet;

use emblem::{Call, Host, Paint, Recorder, Rgba, decode_metadata, execute};
use pictograph::symbols::{
    HOST_SEAL_ENTRY, INK_ENTRY, MUTED_INK_ENTRY, SURFACE_ENTRY, SYMBOL_PALETTE_SPAN, Symbol,
    symbol_palette,
};

#[test]
fn palette_extends_the_face_entries_in_order() {
    let faces = std::array::from_fn(|i| [i as u8, 0, 0, 255]);
    let palette = symbol_palette(
        faces,
        [210, 100, 50, 128],
        [40, 41, 42, 255],
        [50, 51, 52, 255],
        [60, 61, 62, 255],
    );
    assert_eq!(SYMBOL_PALETTE_SPAN, 12);
    for (i, color) in faces.into_iter().enumerate() {
        assert_eq!(palette.0[i], Rgba::new(color[0], 0, 0, 255));
    }
    assert_eq!(palette.0[INK_ENTRY], Rgba::new(105, 50, 25, 128));
    assert_eq!(palette.0[MUTED_INK_ENTRY], Rgba::new(40, 41, 42, 255));
    assert_eq!(palette.0[SURFACE_ENTRY], Rgba::new(50, 51, 52, 255));
    assert_eq!(palette.0[HOST_SEAL_ENTRY], Rgba::new(60, 61, 62, 255));
}

#[test]
fn every_catalog_symbol_matches_its_golden_and_has_two_ink_only_arms() {
    let goldens: [(&str, &[u8]); 8] = [
        ("read", include_bytes!("fixtures/symbols/read.iconvg")),
        ("map", include_bytes!("fixtures/symbols/map.iconvg")),
        ("inspect", include_bytes!("fixtures/symbols/inspect.iconvg")),
        ("trail", include_bytes!("fixtures/symbols/trail.iconvg")),
        ("search", include_bytes!("fixtures/symbols/search.iconvg")),
        ("tabs", include_bytes!("fixtures/symbols/tabs.iconvg")),
        ("fit", include_bytes!("fixtures/symbols/fit.iconvg")),
        ("action", include_bytes!("fixtures/symbols/action.iconvg")),
    ];
    let other = [50, 80, 170, 255];
    let ink = [230, 150, 90, 255];
    let palette = symbol_palette([other; 8], ink, other, other, other);
    let mut seen = HashSet::new();
    for (symbol, (id, golden)) in Symbol::ALL.into_iter().zip(goldens) {
        assert_eq!(symbol.id(), id);
        assert_eq!(Symbol::named(id), Some(symbol));
        let file = symbol.encode().unwrap();
        assert_eq!(file, golden, "{id}");
        assert!(seen.insert(file.clone()), "{id} duplicates another symbol");
        let (metadata, start) = decode_metadata(&file).unwrap();
        assert_eq!(metadata.view_box, emblem::ViewBox::default());
        let run = |height| {
            let mut sink = Recorder::default();
            execute(
                &file,
                start,
                &palette,
                Host {
                    features: 0,
                    height,
                },
                &mut sink,
            )
            .unwrap();
            let fills = sink
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Fill(_)));
            assert!(
                fills.eq([&Call::Fill(Paint::Flat(Rgba::new(
                    ink[0], ink[1], ink[2], 255
                )))]),
                "{id} at {height} should draw exactly once in ink"
            );
            assert!(
                sink.calls
                    .iter()
                    .any(|call| matches!(call, Call::MoveTo(..))),
                "{id} at {height}"
            );
            sink.calls
        };
        let threshold = pictograph::LOD_THRESHOLD;
        let coarse = run(0.0);
        let full = run(threshold);
        assert_eq!(coarse, run(threshold - 0.5), "{id}");
        assert_eq!(full, run(1024.0), "{id}");
        assert_ne!(coarse, full, "{id}");
    }
    assert_eq!(Symbol::named("verified"), None);
}
