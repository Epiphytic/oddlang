# Marmish baseline and inventory

The original is Marmish revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. Its application, original protocol tests, lexical index and Cargo workspace supply independent inputs for qualification.

```sh
scripts/setup-marmish.sh
scripts/qualify-marmish.sh baseline
```

To reuse a pinned checkout, set `ODDLANG_MARMISH_SOURCE` for both commands. Supply an explicit checkout and an empty output directory to retain a named run. Check the stored files with `cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-receipt -- check OUTPUT_DIRECTORY`.

The source tree has 1,641 tracked paths. The upstream ontology covers 418 files and records 15,797 lexical declarations. Its recovered parses, omissions and lexical references limit what can be inferred about runtime reachability. Cargo independently identifies the selected workspace members, targets, features and resolved dependencies. Qualification reports retain these distinct counts.

For this pin the inventory classifies 5,849 lexical declaration sites across the selected targets and feature profiles (including fields and enum variants) as retained native. The compiler type-checked a separate framing consumer. Static module edges resolve to workspace roots. The inventory lists unresolved lexical callees and parser omissions explicitly, so the full runtime denominator stays open. All 33 frozen framing vectors passed against the original. They cover exact JSON bytes, named errors and which check wins when multiple conditions fail.

The baseline runner uses Python 3.11 or later, uv, Git, the pinned Rust toolchain, and native Apple Silicon macOS. The Linux x86-64 and ARM64 entries remain unobserved until their respective hosts run the original and candidate.

| Native platform | Static tests eligible in at least one selected profile | Ignored sites | Runtime baseline |
| --- | ---: | ---: | --- |
| macOS Apple Silicon | 1,199 | 5 | See the archived native run |
| Linux x86-64 | 1,172 | 5 | Unobserved |
| Linux ARM64 | 1,172 | 5 | Unobserved |

Each site has a stable source identifier. Cargo may instantiate the same site more than once. The runtime report records what actually executed. The [inventory units and platform scopes](marmish-inventory.md) explain the count and the packaging inputs. The [framing oracle](marmish-framing-oracle.md) records the canonical base64 behavior of the pinned dependency.

In the default workspace run on Apple Silicon, the top-level Cargo summaries recorded 906 passed, nine failed and five ignored. Six failures occurred in marmish-mdk. Executing the test's copied macOS system binary terminated with SIGKILL in an independent reproduction outside the sandbox, and the shared test mutex subsequently poisoned. Three marmish-native DNS cases needed IP sockets, which the offline baseline denied. These observations identify original behavior under the recorded environment and supply comparison points for the candidate.

The installed-checks run recorded 907 passed, eight failed and three ignored. It overlaps the default suite. Three local DNS failures recur, four socket fixture tests use paths outside the disposable directory, and one benchmark copies the system sleep executable. Independent shell controls reproduced SIGKILL for copied system binaries. The evidence includes the complete command transcripts and the remaining environment restrictions.

The release build completed and hashed all three product binaries and the auxiliary packager. The six CLI help/version probes matched the frozen byte expectations, including the worker's silent rejection of both flags. The [original evidence archive](../../examples/marmish/evidence/original.tar.gz) contains all 22 command records, 44 stdout/stderr logs, the inventory, byte vectors, and environment controls. Its SHA-256 is `4c944d14b9260b2300a164cedda7f8721cb0e487efd1ef7e515c5a900f8fea32`. Extract it to an empty directory and run the receipt checker above.

Claude independently unpacked and replayed the archive at `38809b4`, checked all six CLI probes and approved the macOS baseline. The reported totals exclude filtered subprocess tests nested in an upstream test. The original archive digest anchors subsequent comparisons. The recorded binary digests identify that particular release build. Build directories affect binary bytes.
