# pictograph

Name reservation for **pictograph**, the derived-face generator of the Mere
platform.

A pictograph writes a picture: from a node's content address it derives a
compact vector face — deterministic, so every peer derives the same face for
the same content without shipping an asset. Faces are IconVG bytes (encoded by
[emblem](https://crates.io/crates/emblem)) with palette-indexed fills, so one
blob re-themes at decode time against the current seed palette, and
level-of-detail rides inside the face itself.

The word is the mechanism twice over: a pictograph is a pictorial sign that
carries meaning by convention, and in the statistical sense it is data encoded
as a picture — which is what rule-driven faces are (node kind, state and
degree rendered as visual features, in the Chernoff-glyph tradition).

Lives in the [mere](https://github.com/merely-made/mere) workspace under
`crates/canvas/`. No implementation yet; the plan is
`design_docs/mere_docs/implementation_strategy/2026-08-28_derived_faces_plan.md`.

## License

MPL-2.0 (see LICENSE at the workspace root).
