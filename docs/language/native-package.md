# Native Rust package assembly

`oddlang build native-package rust MANIFEST.json OUTPUT` projects a library from an ordered set of semantic graphs and target overlays. Output must be a fresh path in an existing directory. The command stages inputs and publishes the package after Cargo accepts its locked dependency closure offline. The projection uses the locally installed Cargo and cached registry dependencies.

The format is `oddlang-native-package-v1`. Supply the Cargo package `name`, semantic `version`, `edition` (2021 or 2024), and `dependencies`, keyed by registry crate name. Each dependency specifies a single `version` and optional `features`. Supply `cargoLock` and each component's `graph` and `overlay` as objects containing a relative `path` and lowercase SHA-256 of the exact file bytes in `sha256`. Inputs must resolve inside the manifest directory. The manifest order governs leaf numbering and reexport order.

Each entry in `components` specifies a `kind` (`value-domain`, `endpoint-set` or `json-envelope`), a private Rust `module` name, its pinned input pair and a nonempty list of `reexports`. These exported public structs and enums are available in both their group and the crate root. Each projection occupies a private leaf, allowing siblings to share a group and refer to its public types through `super`. A Turtle projection receives a directory containing only its specified graph as `domain.ttl`. The endpoint projection receives its JSON graph directly.

The generated library includes a Cargo manifest, pinned Cargo.lock and package.lock recording input hashes, projection locks and generated leaf hashes. Build the resulting crate with `cargo build --locked --offline`. Native API and behavior qualification requires a separate consumer.

Run the unrelated image-format exercise from the repository root:

```sh
cargo build --locked --offline --bin oddlang
python3 -B verification/native-package/verify.py target/debug/oddlang
```

It generates two codecs into one private module, exercises the public API from an integration test, compares independently generated directories and rejects stale pins, an inconsistent Cargo lock, invalid or duplicate exports, a symlink escape and a preexisting output.
