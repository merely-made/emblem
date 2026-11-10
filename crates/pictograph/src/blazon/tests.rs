// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use super::*;

#[test]
fn text_round_trips_generated_blazons() {
    let patterns = [
        Pattern::PerPale,
        Pattern::PerFess,
        Pattern::PerBend,
        Pattern::PerChevron,
        Pattern::Barry,
        Pattern::Chequy,
    ];
    let ordinaries = [
        Ordinary::Chief,
        Ordinary::Bend,
        Ordinary::Fess,
        Ordinary::Pale,
        Ordinary::Chevron,
        Ordinary::Bordure,
    ];
    let charges = [
        Charge::Arrow,
        Charge::Cross,
        Charge::Cup,
        Charge::Flame,
        Charge::Key,
        Charge::Leaf,
        Charge::Lozenge,
        Charge::Moon,
        Charge::Mountain,
        Charge::Oak,
        Charge::Sun,
        Charge::Tower,
    ];
    let marks = [
        MarkKind::Label,
        MarkKind::Crescent,
        MarkKind::Star,
        MarkKind::Dot,
        MarkKind::Ring,
        MarkKind::Bar,
    ];
    let mut seen = std::collections::HashSet::new();
    for first in 0..crate::PALETTE_SPAN {
        for second in 0..crate::PALETTE_SPAN {
            for (index, charge) in charges.into_iter().enumerate() {
                let first = T::new(first).unwrap();
                let second = T::new(second).unwrap();
                let variant = index + usize::from(first.index()) + usize::from(second.index());
                let older = Mark(marks[variant % marks.len()], second);
                let newer = Mark(marks[(variant + 1) % marks.len()], first);
                for marks in [
                    Cadency::None,
                    Cadency::One(older),
                    Cadency::Two(older, newer),
                ] {
                    let blazon = Blazon {
                        field: Field::Patterned(patterns[variant % patterns.len()], first, second),
                        ordinary: Some((
                            ordinaries[(index + usize::from(second.index())) % ordinaries.len()],
                            second,
                        )),
                        charge: Some((charge, first)),
                        marks,
                    };
                    let text = blazon.to_string();
                    assert_eq!(text.parse(), Ok(blazon), "{text}");
                    assert!(seen.insert(text));
                }
            }
        }
    }
    assert_eq!(seen.len(), 2_304);
}

#[test]
fn plain_arms_and_the_documented_text_have_exact_forms() {
    for text in [
        "T0",
        "T7, a chief T1",
        "per pale T2 and T5, a cross T1, a label T3",
    ] {
        assert_eq!(text.parse::<Blazon>().unwrap().to_string(), text);
    }
}

#[test]
fn malformed_text_returns_a_typed_error() {
    use ParseBlazonError::*;
    for (text, error) in [
        ("", Field),
        ("per diagonal T2 and T5", Field),
        ("per pale T2", Field),
        ("T8", Tincture),
        ("T01", Tincture),
        ("T２", Tincture),
        ("T1, a chief #ffffff", Tincture),
        ("T1, a mystery T2", Figure),
        ("T1, ", Figure),
        ("T1, a cross T2, a chief T3", FigureOrder),
        ("T1, a chief T2, a bend T3", FigureOrder),
        ("T1, a cross T2, a leaf T3", FigureOrder),
        ("T1, a dot T2, a cross T3", FigureOrder),
        ("T1, a dot T2, a bar T3, a ring T4", TooManyMarks),
    ] {
        assert_eq!(text.parse::<Blazon>(), Err(error), "{text:?}");
    }
}

#[test]
fn structured_serde_round_trips_and_rejects_invalid_slots() {
    let blazon: Blazon = "chequy T2 and T5, a chief T1, a cross T0, a label T3, a dot T4"
        .parse()
        .unwrap();
    let value = serde_json::to_value(blazon).unwrap();
    assert_eq!(value["field"]["Patterned"][1], 2);
    assert_eq!(
        serde_json::from_value::<Blazon>(value.clone()).unwrap(),
        blazon
    );
    let mut invalid = value;
    invalid["marks"]["Two"][0][1] = 8.into();
    assert!(serde_json::from_value::<Blazon>(invalid).is_err());
}
