# emblem

A complete pure-Rust implementation of
[IconVG](https://github.com/nigeltao/iconvg), a compact binary format for
simple vector graphics: icons, logos, glyphs and emoji. It decodes bytecode
into path segments and fills for any 2D backend, and encodes graphics back to
bytes. It does not rasterize.

As far as I can find, it is the only IconVG encoder outside the format
author's Go implementation, and the only complete implementation in Rust.

## The name

This crate is `emblem`, not `iconvg`. The format's name belongs to the
format, and an independent implementation has no business occupying it on a
registry, so `iconvg` is deliberately left unclaimed on crates.io.

*Emblem* is in any case the older word for what this handles: a small
symbolic figure that carries a meaning by convention — which is what an icon,
a logo, a glyph and an emoji each are. The Greek *emblēma* means inlaid work,
a figure set into a larger surface, and that is what the decoder yields: path
segments and fills handed to a host renderer to set into its own scene.

## Status (2026-08-29)

Decoder and encoder for the current register-based IconVG generation, pinned
to spec commit `1864c0573e6b` (the spec last moved 2026-02-27).

- **Decoding** covers the number and coordinate encodings, premultiplied
  colour with palettes and register blending, metadata and ViewBox, and the
  full bytecode VM including jumps, calls, feature detection, and
  level-of-detail branching.
- **Encoding** covers the same ground in reverse: shortest-form numbers,
  colours and registers, geometry runs, whole-file framing, and level-of-detail
  branches. **The encoder reproduces the specification's 36-byte action/info
  icon byte for byte**, and the result decodes to the same call sequence as
  the original.
- **Encoding is deterministic.** The same input always produces the same
  bytes, on every machine and in every process — which is what lets callers
  content-address the output and lets two peers derive an identical file
  independently.
- 157 tests. Every worked example in the specification is a test, from both
  directions.
- Hardened by an adversarial test suite: three reachable panics on crafted
  files were found and fixed (2026-08-08).
- Zero dependencies, unsafe-free, no-std-oriented.

**The format is experimental** (its specification says so verbatim), and this
crate publishes at `0.x` to say the same thing about itself. There are two
IconVG generations: the widely referenced `golang.org/x/exp/shiny/iconvg` and
`google/iconvg` describe the older, simpler one. Files are not interchangeable
between the two.

## Use

### Decoding

`execute` drives a `Sink` with path segments and fills. One rule a backend
must honor: IconVG fills with the non-zero winding rule, never even-odd.

### Encoding

`encode::Writer` builds a graphic and frames it as a file.

```rust
use emblem::ViewBox;
use emblem::encode::Writer;

let mut w = Writer::new(ViewBox { min_x: -24.0, min_y: -24.0, max_x: 24.0, max_y: 24.0 });
w.move_to(0.0, -20.0).unwrap();
w.ellipse(4, (-20.0, 0.0), (0.0, 20.0)).unwrap();
w.fill_flat(8).unwrap();
let bytes = w.finish().unwrap();
```

Two things worth knowing when encoding:

- **Prefer palette references to literal colours.** Registers arrive
  pre-loaded from the palette the *decoder's caller* supplies, so a fill that
  names a register re-themes for free: the same bytes give different colours
  under a different palette. Literal colours opt out of that permanently.
- **`SEL` moves under you.** Ops name a slot, not a register, and address
  `REGS[SEL + slot]`; several ops shift `SEL` as a side effect. `Writer`
  tracks it, and `register_index` tells you where a slot currently points.

## Versions

`0.2.0` is the first release with the encoder, and the first under MPL-2.0.

**`0.1.0` is yanked.** Its decoder started the `SEL` register selector at 0
where the specification says 56, so every register and fill op addressed a
register 56 slots away from the one the graphic named, and colours resolved to
the wrong palette entries on essentially every file that uses a palette. Fixed
in `0.2.0`. That version also shipped under MIT OR Apache-2.0 and keeps that
grant permanently for anyone who took it.

## License

MPL-2.0 (see [`LICENSE`](LICENSE)).

The IconVG specification is Apache-2.0 and belongs to its authors. This is an
independent implementation written from that specification, not a derivative
of anyone's code, so it carries no upstream notice.
