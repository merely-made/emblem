# pictograph

Derives node faces from content addresses: an address in, a compact vector
face out.

A pictograph writes a picture. From a node's content address this derives a
small symmetric mark, encoded as IconVG bytes by
[emblem](https://crates.io/crates/emblem). Faces are 90 to 200 bytes.

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
single bold silhouette when drawn small, the full figure when drawn large. The
decoder chooses; the caller does nothing.

## Use

```rust
let face = pictograph::derive(b"some content address").unwrap();
// `face` is a complete IconVG file. Decode it with emblem against whatever
// palette the current theme derives.
```

`params_of` exposes the parameters an address resolves to — symmetry, form,
the two palette entries, the filled cells — for callers that want to explain a
face rather than only draw one.

## Versioning

`DERIVATION_VERSION` is mixed into the seed, so bumping it changes every face
everywhere. That is a deliberate, visible act, never a side effect of tidying
the code: two peers on different versions would derive different bytes for the
same content and silently disagree. Golden fixtures in the test suite pin the
mapping so a change cannot happen by accident.

Lives in the [mere](https://github.com/merely-made/mere) workspace under
`crates/canvas/`. The plan is
`design_docs/mere_docs/implementation_strategy/2026-08-28_derived_faces_plan.md`.

## License

MPL-2.0 (see LICENSE at the workspace root).
