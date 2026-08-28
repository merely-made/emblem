# emblem

A pure-Rust decoder for [IconVG](https://github.com/nigeltao/iconvg), a
compact binary format for simple vector graphics: icons, logos, glyphs and
emoji. It decodes bytecode into path segments and fills for any 2D backend;
it does not rasterize.

## The name

This crate is `emblem`, not `iconvg`. The format's name belongs to the
format, and an independent implementation has no business occupying it on a
registry, so `iconvg` is deliberately left unclaimed on crates.io.

*Emblem* is in any case the older word for what this decodes: a small
symbolic figure that carries a meaning by convention — which is what an icon,
a logo, a glyph and an emoji each are. The Greek *emblēma* means inlaid work,
a figure set into a larger surface, and that is what the decoder yields: path
segments and fills handed to a host renderer to set into its own scene.

## Status (2026-08-27)

A complete decoder for the current register-based IconVG generation, pinned
to spec commit `1864c0573e6b` (the spec last moved 2026-02-27).

- Covers the number and coordinate encodings, premultiplied colour with
  palettes and register blending, metadata and ViewBox, and the full
  bytecode VM including jumps, calls, feature detection, and
  level-of-detail branching.
- 70 tests; every worked example in the specification is encoded as a test,
  including the 36-byte action/info icon decoding to its printed 7-op
  disassembly.
- Hardened by an adversarial test suite: three reachable panics on crafted
  files were found and fixed (2026-08-08).
- Zero dependencies, unsafe-free, no-std-oriented.

**The format is experimental** (its specification says so verbatim), and this
crate publishes at `0.x` to say the same thing about itself. There are two
IconVG generations: the widely referenced `golang.org/x/exp/shiny/iconvg` and
`google/iconvg` describe the older, simpler one. Files are not interchangeable
between the two.

## Use

For consumers that need compact, scriptless vector icons decoded into a
path stream; the motivating consumer is a graph interface where nodes wear
icon faces, translated onto an existing paint-list renderer. `execute` drives
a `Sink` with path segments and fills. One rule a backend must honor: IconVG
fills with the non-zero winding rule, never even-odd.

## License

MPL-2.0 (see [`LICENSE`](LICENSE)).

The IconVG specification is Apache-2.0 and belongs to its authors. This is an
independent implementation written from that specification, not a derivative
of anyone's code, so it carries no upstream notice.

Published version 0.1.0 shipped under MIT OR Apache-2.0 and keeps that grant
permanently; MPL-2.0 ships at the next functional bump.
