# Assemble recovered public data components

**Goal.** Let ordered native-package manifests consume pinned recovery outputs directly. Keep Cargo and publication policy explicit.

- [x] Exercise a source-free record and linked enum package with compact and explicit component entries, and confirm rejection without publication.
- [x] Resolve a recovery receipt into existing checked kind, graph, Rust overlay, placement and export information.
- [x] Verify native, legacy and real-package comparisons. Record and review the branch.

Each compact component has a `recovery` path-and-SHA pin for its receipt. The receipt names a supported recovery profile and pins `graph.json` and `rust.json` beside it. The native package derives record placement and root exports from the verified record profile. A syntax-only enum receipt does not carry module or root-export evidence. For this profile, supplying `module` explicitly places the enum in the private group and authorizes its crate-root re-export under its derived native type name.

Keep component ordering, package name, version, edition, dependencies, Cargo lock and any unrelated explicit components in the manifest. Use the existing native package generation and linkage checks. The package lock records `recoverySha256` and `recoveredDeclaration` next to the resolved component. Build machines can use the same sealed receipts. Re-recovery on another host can yield a different receipt digest even when native output is byte-identical.

The [macOS ARM64 receipt](../../../verification/recovered-public-data/receipts/aarch64-apple-darwin.json) records native equality for explicit, compact and mixed manifests, 10 rejected inputs, and the 11 unchanged predecessor artifacts. Replay it with `python3 -B verification/recovered-public-data/verify.py PATH_TO_ODDLANG`.
