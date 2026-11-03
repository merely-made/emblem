# Source splits: H5 Emblem receipts

## 2026-11-03: encoder fixtures and VM geometry

Owner: house-style H5 / 01a19f86-cb71-7820-a6ac-aaec39d8bfd4.
Baseline: `1ce843c139575e02826e5893da755fd8b7af3352`. Compact text only, under H5's shared 100 MB cap.

| Original source | Before | After |
|---|---:|---:|
| `src/encode/style.rs` | 686 | 267 |
| `src/encode/writer.rs` | 930 | 396 |
| `src/vm.rs` | 642 | 484 |

All child modules also fit 600 lines. Three exceptions retired, zero remain.
Baseline/post `cargo test --locked`: 157 passed test/doctest cases each.
`prek run --all-files` (ceiling and Clippy) and public docs pass. The VM
retains the same state and instruction bodies; geometry's private helpers
are visible only back to the original VM module. Existing byte-for-byte
encoding and VM conformance fixtures pass. Dependency graph unchanged.

### License-header follow-through

Retained the original source copyright and MPL notices on all three new
modules. Function bodies are unchanged; the actual commit hook rechecks
the ceiling and Clippy. This changes no original-file qualification count.
