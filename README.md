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

## Status: a complete decoder, not a rasterizer

Implemented, and tested against the specification's own worked examples:

| Module | Covers |
|---|---|
| `num` | the natural, coordinate and `float32` encodings |
| `color` | alpha-premultiplied colour, the 128-entry built-in palette, the 64-entry custom palette, the register file, colour-reference resolution and blending |
| `meta` | the magic identifier, metadata chunks, ViewBox, suggested palette |
| `vm` | the bytecode VM: geometry, register, fill, control-flow and reserved ops, including jumps, calls, feature detection and level-of-detail branching |
| `sink` | where a decoded graphic goes: path segments, fills, gradients |

**It decodes; it does not rasterize.** `execute` drives a `Sink` with path
segments and fills, and that is the whole output. IconVG is a presentation
format whose strokes are already flattened to filled paths, so what a consumer
needs is the path stream — which every 2D backend already takes. Rasterizing
here would mean reimplementing, worse, what vello or tiny-skia already do.

One thing a backend must get right: IconVG fills with the **non-zero winding**
rule, never even-odd. A backend defaulting to even-odd renders
self-intersecting icons wrongly, and silently.

62 tests. Where the specification gives a worked example, that example is a
test: the natural number `0x5A 0x83` is 8406; the coordinate `0x82 0x87` is
7.5; `built_in[0x74]` is `C0:80:FF:FF`; the blend in
`REGS[21] = 0x0081_D340_7654_3210` targets `REGS[40]`; and the 36-byte
"action/info" icon decodes to exactly the 7 ops of the disassembly printed
beside it.

## Three rules that are easy to implement plausibly and wrongly

**Jumps count ops, not bytes.** "Ops are atomic and Jumps cannot move the PC
into the middle of a multi-byte op", so a jump has to walk the bytecode
measuring each op. That measurement lives in its own module rather than inside
the interpreter, because two copies of the op-length table would drift.

**An op's length depends on its operands, not just its opcode.** A coordinate
is 1, 2 or 4 bytes depending on its own first byte, so the same cube-to opcode
can be 7 bytes or 25. Measuring by opcode alone lands a jump inside a
coordinate.

**A graphic always terminates, and that is a property of the format.** Jumps
"propel the PC forward (never backward)", and calls "cannot nest — the maximum
call stack depth is one". So there is no fuel counter here and no recursion
limit to tune; a Call while one is outstanding is simply invalid.

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
