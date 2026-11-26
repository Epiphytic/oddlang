# Native regeneration of marmish-stream

The experiment uses the same lossless Rust syntax importer and emitter for
marmish-stream and an independently maintained Cargo package. The
[experiment plan](../superpowers/plans/2026-11-26-generic-rust-roundtrip.md)
defines the checks.

## Frozen baseline

Marmish upstream is `49fc705408a230bd0ad896248d08dbdd2faad05a`. The selected
crate has five tracked Rust files and 2,864 lines. All commands ran on Apple
Silicon macOS using Rust 1.97.1, the upstream Cargo.lock and the crate's default
feature selection.

| Observation | Upstream result |
| --- | --- |
| Cargo test | 29 passed, zero ignored, zero doctests |
| Cargo check with all targets | Passed |
| Cargo release build | Passed |
| Compiler input inventory | 41 test, 41 check and 40 release rustc invocations, each paired with dep-info |
| Distinct inputs in each mode | 348, including all five package Rust sources |
| Independent wire and state corpus | 50 passed |

The [baseline collector](../../scripts/marmish/stream_baseline.py) records
compiler invocations, file hashes, environment dependencies, test module
spans, and named test results. The frozen
[stable receipt](../../examples/marmish/evidence/stream-baseline/stable-original.json.gz)
has uncompressed SHA-256
`91dbfd538635c8bdd92503df9553de761df77560e335fe59011b4d6eadd10514`.
It compares equally after relocating the upstream archive. Rustdoc has no
stable dep-info for its invocation. The receipt records the root and the
empty doctest set. Input accounting for future doctest bodies remains open.
An undeclared
`include_bytes!` input fails the comparison.

The [oracle](../../examples/marmish/stream-driver/README.md) derives
[expected cases](../../examples/marmish/stream-vectors.json) from the wire
format and runs the same downstream program against separately linked builds.
Its [original receipt](../../examples/marmish/evidence/stream-baseline/oracle-original.json.gz)
records all observed bytes and the executable hash. The self-checks detect
original-executable substitution and a build-valid mutation of the Data tag.

The [independent Rust control](../../tests/fixtures/rust-tree-control/README.md)
passed both feature selections before and after renaming its package and
procedural macro dependency alias. It covers a generated Rust unit whose
producer is a build script and a declared text input. The pinned Rust Analyzer
0.0.345 parser recovered all 13 crate and control sources in edition 2024
after recording their BOM and CRLF details. Rust Analyzer documents its
full-fidelity syntax tree. [Parser crate](https://docs.rs/ra_ap_syntax/0.0.345/ra_ap_syntax/)

The [nightly spike](../../examples/marmish/evidence/stream-baseline/nightly-provenance.json)
produced Rustdoc JSON and HIR with nightly-2026-08-21. Its test input inventory
contains 350 distinct paths versus 348 on stable. The differences are
proc-macro2's additional probe and thiserror's `provide.rs`. Compiler-resolved
semantic coverage stays open for the stable build.
