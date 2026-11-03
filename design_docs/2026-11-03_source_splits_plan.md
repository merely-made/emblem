# Source splits: H5 Emblem batch

**Status (2026-11-03):** landed, 3/3 paths qualified.

## Phases and done-conditions

- **S1. Encoder fixtures.** Move existing style-register and geometry-writer
  fixtures into their ordinary test modules, preserving every assertion.
- **S2. VM drawing.** Separate geometry instruction execution from VM control
  flow, keeping its state, arithmetic and private API intact.
- **S3. Qualification.** All three original files and new modules fit 600
  physical lines, retire the exceptions, pass baseline/post tests, Clippy,
  docs and the full local gate, commit and push.

This implements Mark's additional fifty-file request, under Mere's H5.
No new ruling or format change. Total retained receipt cap across H5: 100 MB.

## Findings

- 2026-11-03: the encoder fixtures already form two bounded groups, and the
  VM's geometry instructions have a distinct responsibility from branching,
  registers and fill dispatch. Emblem remains dependency-free.
- Evidence appends to [the receipt](archive_docs/receipts/2026-11-03_source_splits_plan_receipts.md).

## Progress

- 2026-11-03: selected three paths in a clean checkout; baseline started.

- 2026-11-03: S1–S3 landed; three exceptions retired, matching baseline/post tests, docs and full gate passed.
