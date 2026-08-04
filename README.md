# iconvg

A pure-Rust implementation of [IconVG](https://github.com/nigeltao/iconvg), a
compact binary format for simple vector graphics: icons, logos, glyphs and
emoji.

IconVG is deliberately far simpler than SVG. No text, multimedia,
interactivity, linking, scripting, animation, XSLT or DOM. It is a
*presentation* format rather than an authoring one: strokes are flattened into
filled paths before export, so a file is a sequence of "define geometry, define
paint, fill".

## Read this before depending on it

**The format is experimental.** Its specification carries, verbatim:

> WARNING: THIS FORMAT IS EXPERIMENTAL AND SUBJECT TO INCOMPATIBLE CHANGES.

So this crate:

- **pins the specification revision** it implements: `nigeltao/iconvg` commit
  `1864c0573e6b` (spec last touched 2026-02-27);
- **is not published to crates.io.** The `iconvg` name is available there, and
  taking it would advertise a stability that neither the format nor this
  implementation can offer yet. If the format settles, publishing it as
  stewardship for the format's community is the intended path, on the same
  terms as the protocol crates in
  [smolweb](https://github.com/merely-made/smolweb): the name transfers to the
  format's author on request, no conditions.

**There are two IconVG generations, and this is the newer one.** The widely
referenced `golang.org/x/exp/shiny/iconvg` and `google/iconvg` describe an
older, simpler format. The current specification, implemented here, is a
**register-based virtual machine**: 64 registers, a selector, colour-reference
indirection, and control flow. Files are not interchangeable between the two.

## Status: foundation, not a renderer

Implemented, and tested against the specification's own worked examples:

| Module | Covers |
|---|---|
| `num` | the natural, coordinate and `float32` encodings |
| `color` | alpha-premultiplied colour, the 128-entry built-in palette, the 64-entry custom palette, the register file, colour-reference resolution and blending |
| `meta` | the magic identifier, metadata chunks, ViewBox, suggested palette |

**Not implemented:** the bytecode VM — geometry, fill, register and control-flow
ops, including jumps, calls and feature detection. Nothing here rasterizes a
graphic yet, and the crate does not pretend to.

31 tests. Where the specification gives a worked example (the natural number
`0x5A 0x83` is 8406; the coordinate `0x82 0x87` is 7.5; `built_in[0x74]` is
`C0:80:FF:FF`; the blend in `REGS[21] = 0x0081_D340_7654_3210` targets
`REGS[40]`), that example is a test.

## Two things worth knowing about the format

**Colour is alpha-premultiplied everywhere, gradients included.** Halfway
between opaque bright red and transparent black is a 50%-opaque *bright* red,
not a 50%-opaque dark red: hue and saturation survive, only alpha falls. This
differs from SVG.

**Non-sensible colours are meaningful, not errors.** A tuple is "sensible" when
`R <= A`, `G <= A` and `B <= A`. In a register, a *non*-sensible high half is
the tag meaning "this is a blend of two colour references" — so the nonsense is
load-bearing, and resolution deliberately never recurses past one level.

## Why this exists

Vector icons that are compact, resolution-independent, and carry no scripting
surface are a good fit for a graph interface where every node wears a face.
IconVG's model — Bézier geometry filled with flat colours or gradients — maps
directly onto a paint-list renderer, so a decoder needs no new rendering
capability, only a translation.

## License

MIT OR Apache-2.0. The specification is Apache-2.0 and belongs to its authors;
this is an independent implementation of it.
