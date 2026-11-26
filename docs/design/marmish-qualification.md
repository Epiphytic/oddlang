# Whole-workspace Marmish qualification

The experiment recovers two Rust files from Marmish revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. One implements the wire framing contract. The other declares the public core module and its six re-exported type names. Each has a separate semantic graph, source map, import lock, projector and release pin. The assembler regenerates both modules and verifies their bytes against the externally pinned project manifest.

```sh
scripts/setup-http-conformance.sh
scripts/setup-wasm.sh
scripts/setup-marmish.sh
scripts/qualify-marmish.sh rebuild
scripts/qualify-marmish.sh verify
```

The qualifier accepts a pinned Marmish checkout and an empty output directory as optional arguments after the subcommand. `rebuild` leaves a local Git repository in `candidate/`. It recovers and projects twice in separately named directories, creates each project manifest from immutable Git objects, verifies every file mode and SHA-256, and compares the Git trees and commits. Both builds update Marmish's staged source index, create an immutable local commit, and pass the upstream indexer's `check --ref HEAD`. The upstream indexer uses historical release tags. The [tag lock](../../examples/marmish/ontology-tags.lock) identifies each locally supplied Git object.

The framing and façade modules replace two of 1,641 paths. All 1,639 retained paths initially reproduce their original bytes and Git modes. Regenerating the lexical index then changes three of those retained files. The two builds produced candidate commit `e0d855874ec2348939175dfa6ec5c71488c8ba35`, tree `6c0c7b16e3cb7b013a360c1b19e26ed19a238e65`.

The frozen inventory counts 19 regenerated lexical declaration sites and 5,830 retained sites. Field, variant and function sites each contribute to these figures. The remaining 96 runtime-source paths are grouped by missing capability in first-party package dependency order. Call resolution and macro expansion remain incomplete, so the count of compiled runtime declarations is unknown for all nine platform/feature combinations.

`verify` also runs the native suite, the framing component, and mixed consumers linked separately to the untouched original and the committed candidate. The native comparisons record named test outcomes, exact CLI results, platform availability, and source-build provenance. The two wire directions include empty, binary and limit-sized payloads. Authoritative source and candidate commit IDs differ, so release binary hashes identify individual builds.
