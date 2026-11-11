// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Editable source for a heraldic face. The structured serde representation
//! is canonical. The text form is a closed vocabulary for display and paste.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A face palette entry in the range T0 through T7.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct T(u8);

impl T {
    /// Construct a slot from its zero-based palette index.
    pub const fn new(index: u8) -> Option<Self> {
        if index < crate::PALETTE_SPAN {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The zero-based palette index.
    pub const fn index(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for T {
    type Error = ParseBlazonError;

    fn try_from(index: u8) -> Result<Self, Self::Error> {
        Self::new(index).ok_or(ParseBlazonError::Tincture)
    }
}

impl From<T> for u8 {
    fn from(slot: T) -> Self {
        slot.index()
    }
}

impl Display for T {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.index())
    }
}

impl FromStr for T {
    type Err = ParseBlazonError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.as_bytes() {
            [b'T', digit @ b'0'..=b'9'] => Self::try_from(digit - b'0'),
            _ => Err(ParseBlazonError::Tincture),
        }
    }
}

macro_rules! vocabulary {
    ($(#[$doc:meta])* $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word),+
                }
            }

            fn named(word: &str) -> Option<Self> {
                match word {
                    $($word => Some(Self::$variant)),+,
                    _ => None,
                }
            }
        }
    };
}

vocabulary! {
    /// A two-slot field pattern.
    Pattern {
        PerPale => "per pale",
        PerFess => "per fess",
        PerBend => "per bend",
        PerChevron => "per chevron",
        Barry => "barry",
        Chequy => "chequy",
    }
}

/// The ground of a shield.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Field {
    Plain(T),
    Patterned(Pattern, T, T),
}

impl Display for Field {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Plain(slot) => slot.fmt(f),
            Self::Patterned(pattern, first, second) => {
                write!(f, "{} {first} and {second}", pattern.word())
            },
        }
    }
}

impl FromStr for Field {
    type Err = ParseBlazonError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let Some((left, second)) = text.split_once(" and ") else {
            if text.starts_with('T') {
                return Ok(Self::Plain(text.parse()?));
            }
            return Err(ParseBlazonError::Field);
        };
        let (pattern, first) = left.rsplit_once(' ').ok_or(ParseBlazonError::Field)?;
        let pattern = Pattern::named(pattern).ok_or(ParseBlazonError::Field)?;
        Ok(Self::Patterned(pattern, first.parse()?, second.parse()?))
    }
}

vocabulary! {
    /// A broad geometric figure laid over the field.
    Ordinary {
        Chief => "chief",
        Bend => "bend",
        Fess => "fess",
        Pale => "pale",
        Chevron => "chevron",
        Bordure => "bordure",
    }
}

vocabulary! {
    /// The house's central figure.
    Charge {
        Arrow => "arrow",
        Cross => "cross",
        Cup => "cup",
        Flame => "flame",
        Key => "key",
        Leaf => "leaf",
        Lozenge => "lozenge",
        Moon => "moon",
        Mountain => "mountain",
        Oak => "oak",
        Sun => "sun",
        Tower => "tower",
    }
}

vocabulary! {
    /// A member's distinguishing figure.
    MarkKind {
        Label => "label",
        Crescent => "crescent",
        Star => "star",
        Dot => "dot",
        Ring => "ring",
        Bar => "bar",
    }
}

/// A cadency figure and its palette slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Mark(pub MarkKind, pub T);

/// Up to two cadency marks, oldest first.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Cadency {
    #[default]
    None,
    One(Mark),
    Two(Mark, Mark),
}

impl Cadency {
    fn append(self, mark: Mark) -> Result<Self, ParseBlazonError> {
        match self {
            Self::None => Ok(Self::One(mark)),
            Self::One(oldest) => Ok(Self::Two(oldest, mark)),
            Self::Two(..) => Err(ParseBlazonError::TooManyMarks),
        }
    }
}

/// The editable house arms and a member's cadency.
///
/// The text starts with the field, then the ordinary, charge and cadency
/// marks in that order, omitting absent components. For example,
/// `per pale T2 and T5, a cross T1, a label T3`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Blazon {
    pub field: Field,
    pub ordinary: Option<(Ordinary, T)>,
    pub charge: Option<(Charge, T)>,
    pub marks: Cadency,
}

impl Display for Blazon {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.field.fmt(f)?;
        if let Some((kind, slot)) = self.ordinary {
            write!(f, ", a {} {slot}", kind.word())?;
        }
        if let Some((kind, slot)) = self.charge {
            write!(f, ", a {} {slot}", kind.word())?;
        }
        let write_mark =
            |f: &mut Formatter<'_>, Mark(kind, slot)| write!(f, ", a {} {slot}", kind.word());
        match self.marks {
            Cadency::None => Ok(()),
            Cadency::One(mark) => write_mark(f, mark),
            Cadency::Two(oldest, newest) => {
                write_mark(f, oldest)?;
                write_mark(f, newest)
            },
        }
    }
}

/// A syntax error in the closed blazon vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseBlazonError {
    Field,
    Tincture,
    Figure,
    FigureOrder,
    TooManyMarks,
}

impl Display for ParseBlazonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Field => "expected a plain or patterned field",
            Self::Tincture => "expected a palette slot T0 through T7",
            Self::Figure => "expected a figure written as 'a <name> <slot>'",
            Self::FigureOrder => {
                "expected at most one ordinary followed by at most one charge before cadency marks"
            },
            Self::TooManyMarks => "expected at most two cadency marks",
        })
    }
}

impl std::error::Error for ParseBlazonError {}

impl FromStr for Blazon {
    type Err = ParseBlazonError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split(", ");
        let mut blazon = Self {
            field: parts.next().unwrap_or_default().parse()?,
            ordinary: None,
            charge: None,
            marks: Cadency::None,
        };
        for part in parts {
            let (name, slot) = part
                .strip_prefix("a ")
                .and_then(|figure| figure.split_once(' '))
                .ok_or(ParseBlazonError::Figure)?;
            let slot = slot.parse()?;
            if let Some(kind) = Ordinary::named(name) {
                if blazon.ordinary.is_some()
                    || blazon.charge.is_some()
                    || blazon.marks != Cadency::None
                {
                    return Err(ParseBlazonError::FigureOrder);
                }
                blazon.ordinary = Some((kind, slot));
            } else if let Some(kind) = Charge::named(name) {
                if blazon.charge.is_some() || blazon.marks != Cadency::None {
                    return Err(ParseBlazonError::FigureOrder);
                }
                blazon.charge = Some((kind, slot));
            } else if let Some(kind) = MarkKind::named(name) {
                blazon.marks = blazon.marks.append(Mark(kind, slot))?;
            } else {
                return Err(ParseBlazonError::Figure);
            }
        }
        Ok(blazon)
    }
}

#[cfg(test)]
mod tests;
