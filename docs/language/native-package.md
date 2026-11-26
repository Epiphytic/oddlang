# Native Rust package assembly

`oddlang build native-package rust MANIFEST.json OUTPUT` projects a library from an ordered set of semantic graphs and target overlays. Output must be a fresh path in an existing directory. The command stages inputs and publishes the package after Cargo accepts its locked dependency closure offline. The projection uses the locally installed Cargo and cached dependency sources.

The format is `oddlang-native-package-v1`. Supply the Cargo package `name`, semantic `version`, `edition` (2021 or 2024), and `dependencies`, keyed by registry crate name. Each dependency specifies a single `version` and optional `features`. Supply `cargoLock` and each component's `graph` and `overlay` as objects containing a relative `path` and lowercase SHA-256 of the exact file bytes in `sha256`. Inputs must resolve inside the manifest directory. The manifest order governs leaf numbering and reexport order.

An external dependency may select optional `defaultFeatures` and a Git `source` with an HTTPS `git` URL and full lowercase `revision`. A package may also declare `registryPatches`, keyed by crate name, using the same Git source object. Standalone builds apply those pins in `patch.crates-io`. Workspace assembly hoists member pins to the root, unifies identical selections and rejects disagreements before publishing. The root Cargo lock must resolve the resulting combined dependency closure offline.

Each entry in `components` specifies a `kind` (`value-domain`, `endpoint-set`, `json-envelope`, `integrity-block`, `wire-schema`, `wire-protocol` or `shared-declarations`), a Rust `module` name, its pinned input pair and a list of `reexports`. A group defaults to private. All components in a public group declare `visibility: "public"`. The group exposes its `reexports` and exposes only the selected `rootReexports` subset at crate root (default all). Use `crateReexports` for public or crate-visible items needed by sibling groups. At least one group export must be present. Exportable items include constants, structs, enums, functions, modules and type aliases. The group and leaf source files use generated paths, so legal module names such as `lib` are usable on case-insensitive filesystems. Each projection occupies a private leaf, allowing siblings to share a group and refer to its public types through `super`. A Turtle projection receives a directory containing only its specified graph as `domain.ttl`. The endpoint projection receives its JSON graph directly.

A `wire-protocol` component pins its canonical operation graph and target binding with `graph` and `overlay`. It also requires `wireShape`, an object containing separately checked `graph` and `overlay` pins for the structural shape and its target binding. The package lock records all four pins. Use sibling projected shared declarations, codecs, or wire schemas to supply the referenced native external types. The generated module exposes the selected native operation interface through ordinary Rust module reexports.

The generated library includes a Cargo manifest, pinned Cargo.lock and package.lock recording input hashes, projection locks and hashes for every generated source file. Build the resulting crate with `cargo build --locked --offline`. Native API and behavior qualification requires a separate consumer.

Run the unrelated image-format exercise from the repository root:

```sh
cargo build --locked --offline --bin oddlang
python3 -B verification/native-package/verify.py target/debug/oddlang
```

It generates two codecs into one private module, exercises the public API from an integration test, compares independently generated directories and rejects stale pins, an inconsistent Cargo lock, invalid or duplicate exports, a symlink escape and preexisting output. On macOS and Linux it also inserts an empty destination after generation begins and verifies the competing directory stays empty.

A `build-report` component binds its operation IRI to a checked CLI callback. Its fields declare stable identities, wire names and ordered sources. Literal integers declare their unsigned width and lists also declare their length. The projection captures the actual Cargo target in a sealed `build.rs`, reads the package version from Cargo, embeds the graph's represented revision, and emits compact JSON followed by LF. The assembler includes the script in the generated file inventory. Graphs can declare the same ordered argument checks used by local commands.
