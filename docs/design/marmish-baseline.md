# Marmish baseline and inventory

The original is Marmish revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. Its application, original protocol tests, lexical index and Cargo workspace supply independent inputs for qualification.

```sh
scripts/setup-marmish.sh
scripts/qualify-marmish.sh baseline
```

To reuse a pinned checkout, set `ODDLANG_MARMISH_SOURCE` for both commands. Supply an explicit checkout and an empty output directory to retain a named run. Check the stored files with `python3 scripts/marmish/receipt.py OUTPUT_DIRECTORY`.

The source tree has 1,641 tracked paths. The upstream ontology covers 418 files and records 15,797 lexical declarations. Its recovered parses, omissions and lexical references limit what can be inferred about runtime reachability. Cargo independently identifies the selected workspace members, targets, features and resolved dependencies. Qualification reports retain these distinct counts.

For this pin the inventory classifies 5,849 known runtime declarations as retained native. The compiler type-checked a separate framing consumer. Static module edges resolve to workspace roots. The inventory lists unresolved lexical callees and parser omissions explicitly, so the full runtime denominator stays open. All 33 frozen framing vectors passed against the original. They cover exact JSON bytes, named errors and which check wins when multiple conditions fail.

The baseline runner uses Python 3.11 or later, uv, Git, the pinned Rust toolchain, and native Apple Silicon macOS. The Linux x86-64 and ARM64 entries remain unobserved until their respective hosts run the original and candidate.
