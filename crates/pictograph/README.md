# pictograph

Derives node faces from content addresses: an address in, a compact vector
face out.

A pictograph writes a picture. From a node's content address this derives a
small symmetric mark, encoded as IconVG bytes by
[emblem](https://crates.io/crates/emblem). Faces are compact: the current
68-address test corpus spans 34 to 275 bytes, and the suite caps them below
512 bytes.

The word is the mechanism twice over: a pictograph is a pictorial sign that
carries meaning by convention, and in the statistical sense it is data encoded
as a picture — which is what a rule-driven face is.

## Three properties

**Deterministic.** The same address always gives the same bytes, on every
machine and in every process. Every peer derives the same face for the same
content, so a face never has to be shipped, and faces can be content-addressed
themselves.

**Themeable.** Fills name *palette slots*, never literal colours. IconVG
pre-loads its registers from the palette the decoder's caller supplies, and at
the starting `SEL` of 56 the slot `8 + k` addresses custom palette entry `k`.
So a face is re-themed by decoding it against a different palette — no
re-derivation, no stored variants, and no register ops in the file at all.

**Scale-aware.** Each face carries two arms behind a level-of-detail branch: a
coarse 3x3 silhouette when drawn small, the full 5x5 figure when drawn large.
Each coarse cell is 8.1px wide at Canvas's normal 25.92px face height, so the
small arm keeps address-specific geometry instead of collapsing to a coloured
rectangle. Its 68-address default-height corpus has 30 distinct coarse masks;
the largest collision is eight faces, versus 57 faces sharing the old
bounding-rectangle silhouette. The decoder chooses; the caller does nothing.

## Use

```rust
let face = pictograph::derive(b"some content address").unwrap();
// `face` is a complete IconVG file. Decode it with emblem against whatever
// palette the current theme derives.
```

`params_of` exposes the parameters an address resolves to — symmetry, form,
the two palette entries, the filled cells — for callers that want to explain a
face rather than only draw one.

## Vello bridge

Enable the `vello` feature to decode any emblem-supported IconVG file into the
exact `Scene` type consumed by netrender:

```rust
let bytes = pictograph::derive(b"some content address").unwrap();
let graphic = pictograph::vello::decode(
    &bytes,
    &emblem::Palette::default(),
    emblem::Host { height: 64.0, ..Default::default() },
)
.unwrap();
let mut frame_scene = pictograph::vello::Scene::new();
let face_transform = pictograph::vello::kurbo::Affine::translate((32.0, 32.0));
graphic.append_to(&mut frame_scene, Some(face_transform));
```

The bridge carries the ViewBox clip, uses non-zero winding, converts emblem's
premultiplied colours into peniko's straight-alpha inputs, and lowers flat,
linear-gradient, and radial-gradient paints. All four IconVG spread modes are
covered. The dependency is feature-gated so byte-only users do not acquire the
vello scene stack.

## Versioning

`DERIVATION_VERSION` is mixed into the seed, so bumping it changes every face
everywhere. That is a deliberate, visible act, never a side effect of tidying
the code: two peers on different versions would derive different bytes for the
same content and silently disagree. Digest-pinned fixtures in the test suite
record committed byte length, digest, and filled-cell count, so a change cannot
happen by accident.

Lives in the [mere](https://github.com/merely-made/mere) workspace under
`crates/canvas/`. The plan is
`design_docs/mere_docs/implementation_strategy/2026-08-28_derived_faces_plan.md`.

## License

[MPL-2.0](https://github.com/merely-made/mere/blob/main/LICENSE).
