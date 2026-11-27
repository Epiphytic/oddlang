# Script migration inventory

The tab-separated baseline records Git blobs, repository ownership and textual caller candidates. It covers Python, shell, JavaScript, TypeScript, Ruby, Perl and PowerShell files, including consumer examples. Compare the companion with the pinned original before attributing a script to Oddlang. Entries marked `upstream-path-modified` need individual review. A textual reference can be a documented command; dynamic callers require inspection.

Build the inventory tool with `rustc --edition 2024 verification/provider-tooling/inventory.rs -o /tmp/oddlang-tool-inventory` and run it with three clean checkout paths in order: Oddlang, marmish-oddlang and original Marmish. The baseline revisions are `6f9e8475cc4d24ffe67d73c887f3a21e00f21d07`, `51064fa44124b811fdfbe1329cb62de4097eb8de` and `af54339d250290490d62d4475a77539b7e739071`. The third-party OntoGPT skill helpers arrived after this baseline and are distributed under their upstream license.

The [Oddlang root audit](oddlang-root-audit.md) compares the original Oddlang rows with the later tracked tree, includes CJS, TSX and Go consumers, and records the Rust Wasm and hybrid browser qualification results.

The [companion audit](companion-audit.md) links the full path table, upstream comparisons, caller anchors and the remaining migration inventory at the audited companion revision.
