// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::OTHER;

/// One world lattice cell spans 64 units on either axis.
pub const LATTICE: i32 = 64;
/// Number of washes, including the merged remainder.
pub const MAX_CLUSTERS: usize = 16;

type Cell = [i32; 2];

const DIRECTIONS: [Cell; 8] = [
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
    [-1, 0],
    [-1, -1],
    [0, -1],
    [1, -1],
];

/// Content buckets in priority order for resolving a tied histogram.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BackdropKind {
    Document,
    Image,
    Audio,
    Video,
    Data,
    Collection,
    Other,
}

impl BackdropKind {
    const ALL: [Self; 7] = [
        Self::Document,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Data,
        Self::Collection,
        Self::Other,
    ];

    /// Classify a materialized MIME hint and the node's tag set. The tag
    /// `collection` takes precedence over MIME. The other tags are fallback
    /// classes for absent or unrecognized MIME, in histogram order.
    pub fn classify(mime: Option<&str>, has_tag: impl Fn(&str) -> bool) -> Self {
        if has_tag("collection") {
            return Self::Collection;
        }
        if let Some(mime) = mime {
            let essence = mime.split(';').next().unwrap_or_default().trim();
            if let Some((major, subtype)) = essence.split_once('/') {
                if major.eq_ignore_ascii_case("image") {
                    return Self::Image;
                }
                if major.eq_ignore_ascii_case("audio") {
                    return Self::Audio;
                }
                if major.eq_ignore_ascii_case("video") {
                    return Self::Video;
                }
                let is = |value| subtype.eq_ignore_ascii_case(value);
                if is("pdf") || is("epub+zip") || is("xhtml+xml") {
                    return Self::Document;
                }
                let suffix = subtype.rsplit_once('+').map(|(_, suffix)| suffix);
                if is("csv")
                    || is("tab-separated-values")
                    || is("json")
                    || is("xml")
                    || is("rdf")
                    || is("turtle")
                    || is("trig")
                    || suffix.is_some_and(|s| {
                        s.eq_ignore_ascii_case("json") || s.eq_ignore_ascii_case("xml")
                    })
                {
                    return Self::Data;
                }
                if major.eq_ignore_ascii_case("text") {
                    return Self::Document;
                }
            }
        }
        ["document", "image", "audio", "video", "data"]
            .into_iter()
            .zip(Self::ALL)
            .find_map(|(tag, kind)| has_tag(tag).then_some(kind))
            .unwrap_or(Self::Other)
    }
}

/// The cached facts for one anchored member of the view.
#[derive(Clone, Copy, Debug)]
pub struct BackdropNode<'a> {
    pub position: [f32; 2],
    pub kind: BackdropKind,
    pub community: Option<&'a str>,
    pub site: Option<&'a str>,
    /// Held visit time as seconds since the Unix epoch.
    pub visit_seconds: Option<f64>,
}

/// Integer facts sufficient to draw a wash. Retain this as a cache key
/// together with the wash grammar versions.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ViewDigest {
    kinds: [u16; BackdropKind::ALL.len()],
    pub(super) clusters: Vec<ClusterCell>,
    pub(super) density: u8,
    pub(super) activity: u8,
    bounds: [i32; 4],
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct ClusterCell {
    pub center: Cell,
    pub hull: Vec<Cell>,
    pub entry: u8,
}

impl ViewDigest {
    /// Fold held member facts into the wash lattice. Missing or nonfinite
    /// placements contribute no facts. An empty placement set has no wash.
    /// The activity bucket uses the span of these members’ held visits.
    pub fn from_held<'a>(nodes: impl IntoIterator<Item = BackdropNode<'a>>) -> Option<Self> {
        let mut kinds = [0_u16; BackdropKind::ALL.len()];
        let mut groups = BTreeMap::new();
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut total = 0_usize;
        let (mut first_visit, mut last_visit) = (f64::INFINITY, f64::NEG_INFINITY);

        for node in nodes {
            let Some(cell) = snap(node.position) else {
                continue;
            };
            kinds[node.kind as usize] = kinds[node.kind as usize].saturating_add(1);
            total += 1;
            if let Some(visit) = node
                .visit_seconds
                .filter(|visit| visit.is_finite() && *visit >= 0.0)
            {
                first_visit = first_visit.min(visit);
                last_visit = last_visit.max(visit);
            }
            for axis in 0..2 {
                bounds[axis] = bounds[axis].min(cell[axis] - 1);
                bounds[axis + 2] = bounds[axis + 2].max(cell[axis] + 2);
            }
            let group = match (node.community, node.site) {
                (Some(id), _) => (0, id),
                (None, Some(site)) => (1, site),
                (None, None) => (2, ""),
            };
            let category = node.site.map_or(0, |site| {
                (crate::grammar::fnv1a(site.bytes()) % u64::from(OTHER)) as usize
            });
            groups
                .entry(group)
                .or_insert_with(|| Aggregate::new(cell))
                .add(cell, category);
        }
        if total == 0 {
            return None;
        }

        let mut groups: Vec<_> = groups.into_values().collect();
        // The stable sort preserves the ordered map's ID order for ties.
        groups.sort_by_key(|group| Reverse(group.count));
        let mut other = None;
        if groups.len() > MAX_CLUSTERS {
            let mut tail = groups.drain(MAX_CLUSTERS - 1..);
            other = tail.next().map(|mut first| {
                for group in tail {
                    first.merge(group);
                }
                first.cell(OTHER)
            });
        }
        let mut clusters: Vec<_> = groups
            .into_iter()
            .map(|group| {
                let entry = (0..OTHER)
                    .max_by_key(|&i| (group.categories[i as usize], Reverse(i)))
                    .unwrap_or(0);
                group.cell(entry)
            })
            .collect();
        clusters.extend(other);

        let width = i64::from(bounds[2]) - i64::from(bounds[0]);
        let height = i64::from(bounds[3]) - i64::from(bounds[1]);
        // log2 of nodes per 16 lattice cells, rounded down and clamped.
        let area = width as u128 * height as u128;
        let concentration = (total as u128 * 16 / area).max(1);
        let density = concentration.ilog2().min(7) as u8;
        let visit_span_seconds = last_visit - first_visit;
        let activity = [0.0, 3_600.0, 86_400.0]
            .into_iter()
            .filter(|threshold| visit_span_seconds > *threshold)
            .count() as u8;
        Some(Self {
            kinds,
            clusters,
            density,
            activity,
            bounds,
        })
    }

    /// The inclusive lower and exclusive upper edges in lattice coordinates,
    /// ordered `[left, top, right, bottom]`. Multiply by [LATTICE] in world space.
    pub fn bounds(&self) -> [i32; 4] {
        self.bounds
    }

    pub(super) fn dominant_kind(&self) -> BackdropKind {
        BackdropKind::ALL
            .into_iter()
            .enumerate()
            .max_by_key(|(index, _)| (self.kinds[*index], Reverse(*index)))
            .map_or(BackdropKind::Other, |(_, kind)| kind)
    }
}

fn snap(position: [f32; 2]) -> Option<Cell> {
    let cells = position.map(|coordinate| (f64::from(coordinate) / f64::from(LATTICE)).floor());
    cells
        .into_iter()
        .all(|cell| (f64::from(i32::MIN + 1)..=f64::from(i32::MAX - 2)).contains(&cell))
        .then(|| cells.map(|cell| cell as i32))
}

struct Aggregate {
    count: usize,
    sum: [i128; 2],
    support: [Cell; DIRECTIONS.len()],
    categories: [usize; OTHER as usize],
}

impl Aggregate {
    fn new(first: Cell) -> Self {
        Self {
            count: 0,
            sum: [0; 2],
            support: [first; DIRECTIONS.len()],
            categories: [0; OTHER as usize],
        }
    }

    fn add(&mut self, cell: Cell, category: usize) {
        self.count += 1;
        self.categories[category] += 1;
        for (sum, coordinate) in self.sum.iter_mut().zip(cell) {
            *sum += i128::from(coordinate);
        }
        for (best, direction) in self.support.iter_mut().zip(DIRECTIONS) {
            keep_extreme(best, cell, direction);
        }
    }

    fn merge(&mut self, other: Self) {
        self.count += other.count;
        for (sum, coordinate) in self.sum.iter_mut().zip(other.sum) {
            *sum += coordinate;
        }
        for ((best, point), direction) in self.support.iter_mut().zip(other.support).zip(DIRECTIONS)
        {
            keep_extreme(best, point, direction);
        }
    }

    fn cell(self, entry: u8) -> ClusterCell {
        let mut hull = Vec::with_capacity(DIRECTIONS.len());
        for point in self.support {
            if !hull.contains(&point) {
                hull.push(point);
            }
        }
        ClusterCell {
            center: self
                .sum
                .map(|sum| sum.div_euclid(self.count as i128) as i32),
            hull,
            entry,
        }
    }
}

fn keep_extreme(best: &mut Cell, point: Cell, direction: Cell) {
    let score = |[x, y]: Cell| {
        (
            i64::from(x) * i64::from(direction[0]) + i64::from(y) * i64::from(direction[1]),
            x,
            y,
        )
    };
    if score(point) > score(*best) {
        *best = point;
    }
}
