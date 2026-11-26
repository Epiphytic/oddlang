# Nightly Rust definition inventory

`inventory.py` uses the compiler in [rust-toolchain.toml](rust-toolchain.toml) and checks its commit and Rustdoc JSON version. The adapter runs Cargo offline with the workspace lockfile and a fresh target directory. It takes a Cargo workspace root and a package with a documentable library.

```sh
python3 verification/rust-inventory/inventory.py WORKSPACE PACKAGE NEW_OUTPUT
```

Use `--features` and `--no-default-features` for a particular configuration. For an isolated build supply a staged Cargo home, versioned vendor directory and a Seatbelt profile allowing the pinned nightly toolchain. Cargo then runs under that profile. The output includes the actual Cargo commands and logs, a dep-info inventory for `cargo check --all-targets`, raw Rustdoc JSON, and the versioned `rust-inventory.json` receipt.

The definition list covers the documented library with private items for the selected features. It includes Rustdoc's crate-local items, definition kinds and spans when provided, and signature type path references. Rustdoc IDs are scoped to that report. The report records unavailable function-body call targets, body type resolution and macro expansion origins. Inactive code remains unresolved for that configuration. Definition and dep-info scopes are recorded independently because the all-target check covers more Cargo targets than Rustdoc.

A stable build receipt can be supplied with `--stable-receipt`. The comparison requires the same workspace input paths and hashes and lists stable-only and nightly-only inputs outside that set. It also checks the selected manifests and lockfile. The nightly report is evidence about the pinned nightly only. For the stream experiment, the stable and nightly workspace source inputs match, while the nightly toolchain activates two additional third-party Rust files.

Use `--matching-report` for an independently generated candidate. The input digests, configuration and definitions must match the earlier nightly report and the workspace root must differ. The report links the earlier receipt by SHA-256. To bind a receipt to an Oddlang source bundle, supply `--bundle` and `--bundle-sha256`. The adapter verifies the lock, graph, inventory and emitted source digests before publishing the receipt. An authorized graph mutation can be checked against its new lock and inventory without reusing its earlier semantic receipt.
