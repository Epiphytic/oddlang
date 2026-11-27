# Whole-workspace Marmish qualification

The experiment recovers two Rust files from Marmish revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. One implements the wire framing contract. The other declares the public core module and its six re-exported type names. Each has a separate semantic graph, source map, import lock, projector and release pin. The facade importer compiles mutual type-conversion probes to witness type identity. The import lock pins those witnesses by hash. The assembler subsequently regenerates both modules from the pinned releases and verifies their bytes against the externally pinned project manifest.

The published candidate is on `main` in the private Epiphytic repository. GitHub Actions is disabled there because the tree includes Marmish's upstream release workflows. [Qualified source tree](https://github.com/Epiphytic/marmish-oddlang/tree/e0d855874ec2348939175dfa6ec5c71488c8ba35)

```sh
cargo +1.96.1 run --locked --manifest-path verification/http-conformance/Cargo.toml -- setup
cargo run --locked --offline --manifest-path verification/wasm-setup/Cargo.toml
cargo +1.97.1 run --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-setup
cargo +1.97.1 run --quiet --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-qualify -- rebuild
cargo +1.97.1 run --quiet --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-qualify -- verify
```

Run the public commands from the Oddlang repository root on Apple Silicon macOS with the setup dependencies installed. Each run prints its output directory. To retain a named report, pass the frozen Marmish checkout and a new directory, for example `cargo +1.97.1 run --quiet --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-qualify -- verify "$ODDLANG_MARMISH_SOURCE" target/marmish/qualification-local`. The original archive and the native comparison protocol are documented separately. [Frozen baseline](marmish-baseline.md) · [Candidate comparison](marmish-candidate-boundaries.md)

`rebuild` leaves a local Git repository in `candidate/`. It recovers and projects twice in separately named directories, creates each project manifest from immutable Git objects, verifies every file mode and SHA-256, and compares the Git trees and commits. Both builds update Marmish's staged source index, create an immutable local commit, and pass the upstream indexer's `check --ref HEAD`. The upstream indexer uses historical release tags. The tag lock identifies each locally supplied Git object. [Pinned release tags](../../examples/marmish/ontology-tags.lock)

The Rust `marmish-rebuild` command runs both recoveries and checks the receipt against the staged files and immutable Git objects. Pass `--oddlang /absolute/path/to/oddlang` to use a prebuilt compiler. Recheck an extracted `rebuild/` evidence directory without rebuilding the candidate with the command below. A fresh Rust run reproduced all 39 archived witness files byte-for-byte. [Rebuild evidence](../../verification/provider-tooling/marmish-rebuild-rust.json)

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml \
    --bin marmish-rebuild -- --replay /path/to/pinned/marmish /path/to/extracted/rebuild
```

The framing and façade modules replace two of 1,641 paths. All 1,639 retained paths initially reproduce their original bytes and Git modes. Regenerating the lexical index then changes three of those retained files. The two builds produced candidate commit `e0d855874ec2348939175dfa6ec5c71488c8ba35`, tree `6c0c7b16e3cb7b013a360c1b19e26ed19a238e65`.

The frozen inventory counts 19 regenerated lexical declaration sites and 5,830 retained sites. Field, variant and function sites each contribute to these figures. The remaining 96 runtime-source paths are grouped by missing capability in first-party package dependency order. Call resolution and macro expansion remain incomplete, so the count of compiled runtime declarations is unknown for all nine inventory entries. The three targets are `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. Each has entries for `default`, `marmish-engine/fault-injection` and `marmish-native/installed-checks` in `denominators.compiledRuntimeUnits` of the frozen inventory. [Inventory units](marmish-inventory.md)

`verify` also runs the native suite, the framing component, and mixed consumers linked separately to the untouched original and the committed candidate. The native comparisons record named test outcomes, exact CLI results, platform availability, and source-build provenance. The two wire directions include empty, binary and limit-sized payloads. Authoritative source and candidate commit IDs differ, so release binary hashes identify individual builds.

## Replay the archived run

The [qualification archive](../../examples/marmish/evidence/qualification.tar.gz) has SHA-256 `60a0cb9b231d200cb2c03b043f1bd335d5c43fc362b17352fd5ce7017e4d425a`. The native observations originated in a clean Oddlang checkout at `957433a`. Two fresh rebuilds at `481bec7` supplied the Git object witnesses for that same immutable candidate. The comparator and aggregate verifier at `bccd6c3` replayed the observations. The Rust replay accepts the complete framing harness identities from that historical revision or a freshly collected harness from the checkout. Replay trusts the Oddlang checkout that supplies the verifier, the assembly and emitter release pins, and the frozen inventory and original observation archive. The lexical ontology checks ran during each build. Replay binds those ontology blobs to their Git tree, commit and check logs. Keep the extracted directory for the named comparisons, individual logs and compiler input receipts.

```sh
archive=examples/marmish/evidence/qualification.tar.gz
printf '%s  %s\n' 60a0cb9b231d200cb2c03b043f1bd335d5c43fc362b17352fd5ce7017e4d425a "$archive" | shasum -a 256 -c -
report=$(mktemp -d)
tar -xzf "$archive" -C "$report"
cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-phase7 -- replay "$report"
```

## Observed reconstruction and interoperability

| Measurement | Upstream source | Reconstructed candidate |
| --- | --- | --- |
| Immutable commit | `49fc705408a230bd0ad896248d08dbdd2faad05a` | `e0d855874ec2348939175dfa6ec5c71488c8ba35` in two independent rebuilds |
| Tracked paths | 1,641 | 1,641, with 2 regenerated Rust files and 3 refreshed lexical index files |
| Framing vectors | 33 upstream expectations | 33 cases exercised by native and instantiated component consumers |
| Mixed wire direction | Original writer → candidate reader, 3 exchanges | Candidate writer → original reader, 3 exchanges |
| Compiler-resolved runtime definition count | Unknown | Unknown |
| Linux x86-64 and ARM64 native execution | Unobserved | Unobserved |

The `rebuild/rebuild.json` receipt records the locked assembly digest, each generated source blob and the immutable commit. `wire.json` records the two native API consumers and both wire directions. `frame/observations/report.json` records the original, regenerated and mutant comparisons. The framing corpus classifies behavior as tested. Exhaustive whole-application equivalence remains open.

## Native platform comparison

| macOS Apple Silicon source-build obligation | Frozen original | Candidate |
| --- | --- | --- |
| Workspace tests | 906 passed, 9 failed, 5 ignored | Identical outcomes for all 920 named tests |
| Installed-feature tests | 907 passed, 8 failed, 3 ignored | Identical outcomes for all 918 named tests |
| Three release binaries | Built | Built, with source-build SHA-256 for each binary |
| Help and version for each binary | Six recorded exit statuses and stdout/stderr byte streams | All six matched exit, stdout and stderr |

The comparison covered all 22 frozen commands. Both suites preserved every named test outcome and had zero improved command exits. The SDK diagnostic contained run-specific cache suffixes. Its comparison permits the recorded suffix variation. The additional candidate-only inactive-installation status scenario matched its expected output and preserved the fixture state.

The original failures include three DNS fixture cases in the denied-network environment. The full list records test names for both suites. The release hashes identify the bytes of individual builds. Packaging, live multi-host installed scenarios, and Linux native execution each require their own evidence. [Frozen named outcomes](../../examples/marmish/evidence/original-results.json)

## Evidence checks

Run the focused negative controls from the repository root.

```sh
cargo +1.97.1 test --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-phase7
cargo test --locked --offline --test workspace_assembly
```

Nine evidence checks passed. They reject a deleted generated file, a changed wire digest, a retained-file digest differing from the original inventory, an omitted platform row, and an incomplete wire direction or case set. They also exercise a contradictory native exit status, a retained site outside an inventoried package, independently checked second-build artifacts, and coordinated rewrites of the candidate Git tree and commit. All ten assembler tests passed, including compilation after deleting the original fixture inputs and store. Separate tests reject swapped producers and internally consistent rewritten facade receipts or locks.

## Remaining language coverage

| Capability needed for recovery | Retained runtime-source files | Frozen lexical declaration sites |
| --- | ---: | ---: |
| General Rust bodies and native dependency resolution | 86 | 5,187 |
| Platform-specific calls and observable OS effects | 5 | 254 |
| Numeric wire tags and checked binary cursors | 2 | 311 |
| Configuration and URL normalization | 1 | 23 |
| Offset arithmetic and ordered window transitions | 1 | 37 |
| Bounded compression primitive | 1 | 18 |
| **Total** | **96** | **5,830** |

The Phase 6 framing pilot left 97 files and 5,832 lexical sites. Recovering the two declaration sites in the core facade brings the Phase 7 residual register to 96 files and 5,830 sites. All 96 files currently use retained native implementations. The gap register specifies a smaller fixture and a rejection condition for each group. The native source remains responsible for storage, credential, transport and process effects. Per-platform counts of compiled retained and unsupported definitions remain unknown until call resolution and macro expansion cover the selected Cargo builds. The generated `coverage.json` orders residual files by first-party package dependencies. [Gap register](../../examples/marmish/gaps.json) · [Assembly contract](../language/workspace-assembly.md)

## Independent review

Claude reviewed commit `957433a` before the native run finished and requested changes. The review identified missing negative evidence controls, insufficient checks on the native verdict and platform map, a stale residual register, and a need to link the task status and numerical claims to the archived receipts. It also flagged a duplicate snapshot helper and asked the native comparator to identify improved test outcomes when a failing suite exits successfully. The first five evidence-control tests passed after that review. The summary now recomputes the native comparison and validates the platform map. The outer command rejects a regression exit even when a comparison report exists. The current register removes the recovered facade, the inventory identifies each target/configuration pair, and the native comparator has one snapshot implementation.

A second review at `0c40822` confirmed the source-site counts and rejected substitutions of generated code, locks, modes, retained digests and ontology logs. It also reproduced a coordinated rewrite of the claimed candidate commit that passed the then-current receipt comparison. The reviewer called for recomputation from Git objects, an explicit list of improved command exits, and a sealed-archive replay after native execution. Commit `fd78d8e` binds the commit and tree IDs to Git object bytes and reports improved native outcomes by name. The new coordinated-rewrite regression test passed. Claude approved the sealed archive and scoped claims at `38944e9`. Its replay rejected changes to the generated code, wire bytes, platform rows, native verdict, retained digest and Git objects in both builds. A coordinated rewrite that also recomputed the candidate Git objects failed the independent native provenance check. The reviewer accepted the baseline failures and unobserved Linux platforms as reported. The separate clean public verifier at `bccd6c3` reproduced the candidate commit and returned `baseline failures remain` with zero new regressions. Its aggregate receipt SHA-256 was `e875b6be85e8747b5d1b4477d08f69e42973620a61c6db06eefd1a845d5c09d0`.
