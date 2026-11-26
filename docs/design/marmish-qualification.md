# Whole-workspace Marmish qualification

The experiment recovers two Rust files from Marmish revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. One implements the wire framing contract. The other declares the public core module and its six re-exported type names. Each has a separate semantic graph, source map, import lock, projector and release pin. The facade importer compiles mutual type-conversion probes to witness type identity. The import lock pins those witnesses by hash. The assembler subsequently regenerates both modules from the pinned releases and verifies their bytes against the externally pinned project manifest.

```sh
scripts/setup-http-conformance.sh
scripts/setup-wasm.sh
scripts/setup-marmish.sh
scripts/qualify-marmish.sh rebuild
scripts/qualify-marmish.sh verify
```

Run the public commands from the Oddlang repository root on Apple Silicon macOS with the setup dependencies installed. Each run prints its output directory. To retain a named report, pass the frozen Marmish checkout and a new directory, for example `scripts/qualify-marmish.sh verify "$ODDLANG_MARMISH_SOURCE" target/marmish/qualification-local`. The original archive and the native comparison protocol are documented separately. [Frozen baseline](marmish-baseline.md) · [Candidate comparison](marmish-candidate-boundaries.md)

The qualifier accepts a pinned Marmish checkout and an empty output directory as optional arguments after the subcommand. `rebuild` leaves a local Git repository in `candidate/`. It recovers and projects twice in separately named directories, creates each project manifest from immutable Git objects, verifies every file mode and SHA-256, and compares the Git trees and commits. Both builds update Marmish's staged source index, create an immutable local commit, and pass the upstream indexer's `check --ref HEAD`. The upstream indexer uses historical release tags. The [tag lock](../../examples/marmish/ontology-tags.lock) identifies each locally supplied Git object.

The framing and façade modules replace two of 1,641 paths. All 1,639 retained paths initially reproduce their original bytes and Git modes. Regenerating the lexical index then changes three of those retained files. The two builds produced candidate commit `e0d855874ec2348939175dfa6ec5c71488c8ba35`, tree `6c0c7b16e3cb7b013a360c1b19e26ed19a238e65`.

The frozen inventory counts 19 regenerated lexical declaration sites and 5,830 retained sites. Field, variant and function sites each contribute to these figures. The remaining 96 runtime-source paths are grouped by missing capability in first-party package dependency order. Call resolution and macro expansion remain incomplete, so the count of compiled runtime declarations is unknown for all nine inventory entries. The three targets are `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. Each has entries for `default`, `marmish-engine/fault-injection` and `marmish-native/installed-checks` in `denominators.compiledRuntimeUnits` of the frozen inventory. [Inventory units](marmish-inventory.md)

`verify` also runs the native suite, the framing component, and mixed consumers linked separately to the untouched original and the committed candidate. The native comparisons record named test outcomes, exact CLI results, platform availability, and source-build provenance. The two wire directions include empty, binary and limit-sized payloads. Authoritative source and candidate commit IDs differ, so release binary hashes identify individual builds.

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
| Installed-feature tests | 907 passed, 8 failed, 3 ignored | Comparison pending |
| Three release binaries | Built | Comparison pending |
| Help and version for each binary | Six recorded exit statuses and stdout/stderr byte streams | Comparison pending |

The original failures include three DNS fixture cases in the denied-network environment. The full list records test names for both suites. The release hashes identify the bytes of individual builds. Packaging, live multi-host installed scenarios, and Linux native execution each require their own evidence. [Frozen named outcomes](../../examples/marmish/evidence/original-results.json)

## Evidence checks

Run the focused negative controls from the repository root.

```sh
python3 -B -m unittest discover -s scripts/marmish -p 'test_phase7.py' -v
cargo test --locked --offline --test workspace_assembly
```

Five evidence checks passed. They reject a deleted generated file, a changed wire digest, a retained-file digest differing from the original inventory, an omitted platform row, and an incomplete wire direction or case set. All ten assembler tests passed, including compilation after deleting the original fixture inputs and store. Separate tests reject swapped producers and internally consistent rewritten facade receipts or locks.

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

Claude reviewed commit `957433a` before the native run finished and requested changes. The review identified missing negative evidence controls, insufficient checks on the native verdict and platform map, a stale residual register, and a need to link the task status and numerical claims to the archived receipts. It also flagged a duplicate snapshot helper and asked the native comparator to identify improved test outcomes when a failing suite exits successfully. The five negative checks above passed after the first review. The summary now recomputes the native comparison and validates the platform map. The outer command rejects a regression exit even when a comparison report exists. The current register removes the recovered facade, the inventory identifies each target/configuration pair, and the native comparator has one snapshot implementation.

A second review at `0c40822` confirmed the source-site counts and rejected substitutions of generated code, locks, modes, retained digests and ontology logs. It also reproduced a coordinated rewrite of the claimed candidate commit that passed the then-current receipt comparison. The reviewer called for recomputation from Git objects, an explicit list of improved command exits, and a sealed-archive replay after native execution. The final review and archive verification gates remain open.
