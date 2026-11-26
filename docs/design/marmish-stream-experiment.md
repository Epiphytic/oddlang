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

## Regenerated release

The Marmish Oddlang repo stores the five stream sources in a single
[pinned Rust syntax graph](https://github.com/Epiphytic/marmish-oddlang/blob/d10dd6e4955ef6876c2127a82561523a6e5d97bd/oddlang/stream/syntax.ttl).
The source-free release tree is
[`7eca9d87ae696ba7cc790f735c7e596618f4d317`](https://github.com/Epiphytic/marmish-oddlang/tree/7eca9d87ae696ba7cc790f735c7e596618f4d317).
The recipe pins the compiler source at
[`f5f6ea6`](https://github.com/Epiphytic/oddlang/tree/f5f6ea6).
The independently pinned bundle lock digest is
`f936373f48aae38e67302a425737e2ceaa888a2a404442399ace21d58925d709`.
The consuming repository retains the original Cargo manifest and lock.
Its bootstrap materializes the crate at its existing path with the same
name, version, dependencies and Rust bytes. No package code was added to
the generic importer or emitter for this experiment.

The [qualification runner](../../scripts/marmish/qualify_stream_workspace.py)
unpacks a Git archive in a fresh directory. Separate Seatbelt profiles
isolate the graph emitter, repository bootstrap, and native Cargo build.
Permission probes cover the upstream checkout, Git object data from both
repositories, a prior artifact, and an undeclared file. The build sees
vendored locked dependencies and the emitted workspace. The emitter only
sees its graph inputs, executable and scratch directory.

The isolated build reproduced all 29 named stream tests and their four
inline module spans. It passed the all-targets check and release build,
matching 122 stable rustc invocations and all 348 distinct compiler inputs
in each mode. Removing the emitted `copy_protocol.rs` caused the expected
missing-module failure. An undeclared `include_bytes!` input failed
inventory reconciliation. The candidate workspace passed the all-targets check, and all six
selected engine, prototype and native test cases passed on both sides.
They cover descriptor ownership, bounded framing, copy ordering, one-block
encoding, and two stream acknowledgement paths. The broader exploratory
consumer runtime sweep encountered sandbox-related failures. The final
report records the six named cases that qualify the drop-in integration.

The [qualification archive](../../examples/marmish/evidence/stream-qualification/qualification.tar.gz)
contains the stable candidate receipt, exact named downstream results,
source-access denials, native toolchain identity, and both wire receipts.

Both independent wire runs passed all 50 cases and nine mixed wire replays
per direction. Each executable records its binding to the package
sources, role, workspace provenance and freshly built library hashes.
A reauthorized graph changed one Data tag from 5 to 12. That graph built
under isolation and failed exactly the two encoding cases in the
[independent mutation check](../../scripts/marmish/stream_mutation.py).

The separately pinned [nightly adapter](../../verification/rust-inventory/README.md)
matched 1,247 Rustdoc definitions and 41 compiler invocations across the
original and candidate. It reports declaration references. Function-body
types, call targets and macro expansion origins remain unresolved for
this experiment. The unrelated control showed a generated build-script
input and different active files under the two feature configurations.
All eight of its source files regenerated byte for byte in both naming
variants. Both independent tests passed in all four candidate builds.

Claude CLI reviewed each phase in auto mode. Baseline revisions bound
workspace inputs and tested executable identities, fixed inline test spans,
and accounted for declared build-script producers. The syntax review
tested reauthorized malformed graphs. The build review identified nested
test output in the consumer harness. The revised runner executes one
selected test per process and checks its outer outcome. The full standard
Oddlang suite passed 144 tests. Its separate Rust control integration
test passed when explicitly selected.

## Remaining Marmish migration

The production syntax adapter parsed and reconstructed 192 of 192 tracked
Rust files in the pinned upstream revision, including the two standalone
tools. This sweep exercised the adapter and byte reconstruction for each
file. The adopted stream crate separately passed graph emission and Cargo
qualification. The remaining workspace has six crates and 185 crate files,
plus the two tools.

| Remaining crate | Rust paths | Lines | Main qualification work |
| --- | ---: | ---: | --- |
| marmish-core | 4 | 442 | Adopt the native bundle alongside the existing behavioral graphs |
| marmish-mdk | 3 | 682 | Verify dependency configuration and native build |
| marmish-engine | 40 | 18,641 | Shard the graph and cover optional features and integration tests |
| marmish-prototype | 3 | 1,256 | Verify downstream linkage and the prototype CLI |
| marmish-vault | 9 | 2,633 | Account for storage fixtures and locked native dependencies |
| marmish-native | 126 | 104,251 | Shard the graph, regenerate build.rs, inventory assets, and qualify installed binaries |
| Standalone tools | 2 | 269 | Support explicit Rust inputs for both Cargo and direct rustc targets |

The stream graph is approximately 18 MiB. Extrapolating from measured syntax
node counts puts the engine graph near 111 MiB and the native graph near
632 MiB in the current canonical format. Both exceed GitHub's 100 MiB
per-file limit for regular Git blobs. [GitHub file limits](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)

Use a pinned manifest of file graphs and bound encoder and
emitter memory by processing shards. The local package capture already
handles macros, dependencies, tests and build.rs through the standard
Cargo pipeline.

Estimate for one engineer on the same Apple Silicon target is **7–13 working
days**.

- **2–4 days** for canonical source-file shards and a release lock covering
  all bundles.
- **2–4 days** for atomic multi-package assembly, explicit inputs for the
  two tools, and build producer and asset closure.
- **3–5 days** for source-free release and installed CLI qualification,
  including native dependency staging and replay of the existing tests.

Budget **3–7 additional days** for Linux runners, system libraries, and
platform-specific packaging and test evidence. The estimate assumes the
pinned third-party Rust and native dependencies and non-Rust workspace
assets remain available. It covers rebuilding and qualifying all tracked
first-party Rust from its graphs. A compiler-resolved or portable behavioral
model for the application would need a separate estimate.
