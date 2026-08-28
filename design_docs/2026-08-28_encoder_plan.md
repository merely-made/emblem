# Encoder Plan

**Date:** 2026-08-28
**Status:** plan.
**Scope:** add an encode side to `emblem`, making it a complete IconVG
implementation: declarative input in, deterministic `.ivg` bytes out. The
motivating consumer is mere's derived node faces
(`repos/mere/design_docs/mere_docs/implementation_strategy/2026-08-28_derived_faces_plan.md`),
which also fixes the two format features the encoder must not lose: the
decoder-input palette and level-of-detail branching.

## Why an encoder, and why here

No Rust encoder exists for *any* compact binary vector format — `emblem` and
the `tinyvg` crate are both decode-only, and IconVG's only encoder anywhere is
the format author's Go implementation. Icons for the family's apps, and
procedurally derived node faces, both need to *produce* `.ivg`; this crate
already holds the decoder, the pinned spec revision, and the conformance
vectors, so the encode side belongs beside them. A complete implementation is
also the stronger stewardship posture recorded in the README.

## Prior-art positioning (assessed 2026-08-28)

- **IconVG is a compilation target by design.** Its author's own workflow is
  SVG → Go converter → `.ivg`; nobody authors the bytecode directly. This plan
  keeps that grain: authoring happens on a declarative middle, and this
  encoder is the compiler.
- **HVIF (Haiku Vector Icon Format)** is the existence proof that compact
  binary vector icons and real authoring coexist — declarative
  styles/paths/shapes tables co-designed with its editor (Icon-O-Matic),
  per-shape level-of-detail, icons small enough for inode attributes. Its
  lesson transfers without its format: the authoring structure need not be the
  wire structure. Our input model plays HVIF's declarative role; IconVG
  bytecode plays the wire role.
- **COLRv1 + CPAL** (color fonts) validate render-time palette substitution as
  mainstream practice; IconVG's caller-supplied palette is the same mechanism.
- **TinyVG** is the simpler-by-design alternative and was considered:
  rejected because it lacks the decoder-input palette and LOD — the two
  features doing load-bearing work for the consumer — and has no Rust encoder
  either.

## Format identity: implementation now, derivative by decision, never by drift

Ruled with Mark 2026-08-28, answering his reframe question ("is emblem better
thought of as a reference implementation of a derivative or superset of
IconVG?"). The answer: not yet, and when yes, deliberately.

- **Today emblem is a faithful implementation**, and faithfulness is an asset
  with a real exchange rate: the spec's own worked examples as a byte-exact
  conformance suite, and the format author's Go encoder as a differential
  oracle, both work only while we match the spec.
- **Two escape valves cover foreseeable superset wants without forking.**
  Unknown metadata MIDs are skipped by conforming decoders (`src/meta.rs:102`,
  tested), so a private chunk — e.g. derivation provenance: generator version
  and seed — rides inside a file every conforming decoder still draws. And
  the declarative middle is where design sovereignty actually lives: the wire
  format is an output detail behind the encoder API, so consumers survive a
  wire change untouched.
- **The fork trigger is recorded, not pre-committed:** if the EXPERIMENTAL
  spec moves incompatibly again (it has once — the two generations), the
  choice is made then: track upstream, or freeze at our pin plus extensions.
  That freeze, if taken, is the birth of the emblem *format*, as a dated
  decision. The crate is already named for that outcome, and `iconvg` stays
  unclaimed on the registry under both futures.
- The first feature that would genuinely force a fork is stroke support
  (fill-only is the format's deepest structural commitment). Icons do not
  need it; kurbo expands strokes upstream.

## Design

### Input: a declarative middle, never SVG

The encoder's input is resolved geometry and styling, in the vocabulary the
rest of the stack already speaks (kurbo/peniko live throughout the lattice via
vello):

- paths as cubic/quadratic/line segments (`kurbo::BezPath`-shaped; whether the
  API takes kurbo types directly or a minimal owned mirror is an E1 decision —
  a mirror keeps the crate zero-dependency, which the decoder currently is);
- fills whose color is a **color source**: a literal RGBA *or a palette index
  (0..=63)*. This distinction is the point of the whole exercise — an encoder
  that only emits literal colors produces compact files with no advantage over
  a static path table, because render-time theming dies. The API must make
  palette-indexed the natural spelling, not the expert option;
- gradients (linear/radial, spread, ≤64 stops) whose stops are color sources
  too;
- ViewBox and the suggested-palette metadata chunk;
- an optional **LOD split**: the same logical icon supplied as two arms
  (below/above a threshold), emitted as the spec's level-of-detail branch.

SVG parsing, CSS, transforms-flattening and stroke expansion are explicit
non-goals: `usvg` and `kurbo::stroke` solve those upstream, and a generator
consumer never has them in the first place.

### Emission: a disciplined subset

v1 emits a simple linear program: metadata, ViewBox, style register setup,
path ops, fills, with at most the one LOD branch. No calls, no general jumps,
no register-allocation cleverness. Like most PDF writers, the encoder uses a
well-chosen subset of the instruction set; the decoder must accept the full
spec, the encoder need only produce valid, reasonably compact files.
`Call`-based motif reuse (attractive for generators, which repeat motifs) is
recorded as a possible E5, gated on a measured size win in the consumer.

### Byte-determinism is a hard requirement

Same input → same bytes, across runs, processes and machines. The consumer
derives faces from content addresses and peers must derive *identical* blobs;
face blobs may themselves be content-addressed. Concretely: no `HashMap`
iteration anywhere in the emission path, stable ordering rules stated in doc
comments, and a CI-style test that encodes the corpus twice in-process and
compares byte-for-byte. Retrofitting this is miserable; it is designed in from
E1.

### Conformance and the oracle

The decoder's test suite already encodes every worked example in the
specification, including the 36-byte action/info icon and its 7-op
disassembly. The encoder inherits that as its acceptance bar from the other
side, plus round-trip properties against our own decoder. The format author's
Go encoder is available as an optional *differential* oracle (compare bytes
for the same logical icon), never as a build dependency.

## Phases

### E1. Number, coordinate and color encoders

The write-side inverses of `num.rs` and `color.rs`: natural, real and
coordinate encodings with correct shortest-form selection, premultiplied
color encoding, palette-index references.

Done when:
- round-trip property tests against the decoder pass over the full value
  ranges (encode → decode → identical value, or documented quantization);
- every encoding example printed in the specification is reproduced
  byte-exactly;
- the determinism harness exists and passes on this layer.

### E2. Styling and register emission

CREG/NREG setup, fill styling from color sources, gradient emission.

Done when:
- fills through both literal and palette-indexed sources decode (via our
  decoder) to the expected `Paint`, with palette substitution verified by
  decoding the same bytes against two palettes and observing both resolutions;
- gradient emission round-trips stops, spread and kind through the decoder.

### E3. Paths, ops, and whole-file assembly

Path segment emission, the op stream, magic + metadata + ViewBox framing;
public `encode` entry point.

Done when:
- encoding the action/info icon's geometry reproduces **the specification's
  36 bytes exactly**;
- every spec worked example round-trips: our decoder's recorded call sequence
  from the encoded bytes equals the sequence from the spec's own bytes;
- a randomized path/fill fuzz round-trips through decode with no panics and
  value-equal geometry;
- encoding the corpus twice yields identical bytes.

### E4. The LOD arm

Two-arm input emitted as a level-of-detail branch.

Done when a two-arm icon decodes to different call sequences under different
host LOD values, and to the correct single sequence at each extreme.

### E5 (recorded, not committed): `Call`-based motif reuse

Only if the derived-faces consumer measures a worthwhile size win.

## Non-goals

- SVG parsing or any styling resolution (usvg's job, upstream);
- stroke expansion (kurbo's job, upstream);
- an optimizing encoder (smallest-possible output);
- authoring UI of any kind;
- the older `golang.org/x/exp/shiny` IconVG generation;
- publishing `iconvg` on crates.io (standing decision, README).

## Versioning and license

The encoder ships in the next functional bump (`0.2.0`), which per the
license sweep plan's invariant 8 is also the version that carries MPL-2.0 to
crates.io. The spec pin (`1864c0573e6b`) is unchanged; if the EXPERIMENTAL
spec moves, re-pinning is its own dated decision, not a side effect.

## Findings

- 2026-08-28: `execute` takes the palette as a decode-time argument
  (`src/vm.rs:74`) and `Paint` reaches the sink fully resolved
  (`src/sink.rs:99`). Two consequences recorded: render-time theming works by
  re-decoding against a new palette (cheap at icon sizes), and
  decode → edit → re-encode silently loses palette indirection, so any future
  editing surface must operate on the declarative middle, never on decoded
  bytes.
- 2026-08-28: no Rust encoder exists for IconVG or TinyVG (both crates
  decode-only, verified on crates.io); the only IconVG encoder anywhere is the
  format author's Go implementation.
- 2026-08-28: unknown metadata MIDs are skipped, not refused (`src/meta.rs:102`,
  test `an_unknown_mid_is_skipped_rather_than_refused`), and chunks must be in
  increasing MID order without repeats. Consequence: private metadata is a
  *conforming* extension path, so most superset wants need no fork; also any
  private MID we mint must respect the ordering rule relative to MID 8 and 16.

## Progress

- 2026-08-28: plan written; design ruled with Mark (encoder + derivation
  first, editing deferred; authoring on the declarative middle; the
  prior-art question assessed — HVIF, COLRv1/CPAL, TinyVG — and recorded
  above).
