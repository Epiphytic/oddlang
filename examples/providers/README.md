# Replace a public-record provider

The two workspace manifests generate the same public Rust type from the hand-authored graph and overlay in `fixture/`. The Wasm manifest pins the path and SHA-256 of `registry.json`. The registry pins component bytes and schemas for contract version `1`. Each registration names its WIT world and supplies its configuration. The separate provider receipts in `receipts/` record the component, schema, inputs and output digests.

From the repository root, run both workspace builds into fresh directories.

```sh
cargo run --locked --offline -- build native-workspace rust examples/providers/workspace-native.json /tmp/records-native
cargo run --locked --offline -- build native-workspace rust examples/providers/workspace-wasm.json /tmp/records-wasm
```

For standalone builds, use `--providers examples/providers/registry.json` after the destination on `build public-records rust` or `recover public-root-records`. The recovery example provides one source under the logical name `source.rs`. The guest reads that source and returns an ordered graph, a Rust overlay, source evidence and a gap for native binding verification. The example guest supports flat root records with named public boolean, u32 and u64 fields. The built-in retains its broader Rust record profile.

Two independent capabilities share the generation world. The field-list provider emits a text file from a list in the input document. The library-binding provider requires an IRI in the declared `urn:example:` namespace and binds it to the Rust type selected in its registration. Run them with the same host executable.

```sh
cargo run --locked --offline -- provider run generate example.field-list 1 examples/providers/registry.json examples/providers/fixture/fields-input.json /tmp/provider-fields
cargo run --locked --offline -- provider run generate example.library-binding 1 examples/providers/registry.json examples/providers/fixture/binding-input.json /tmp/provider-binding
```

Run the integration tests to compare recovery results with the hand-authored graph and overlay, check the receipts, and compile the same independent consumer against both standalone projections and both generated workspace packages.

```sh
cargo test --locked --offline --test provider_interop
```

The checked-in components were built with Rust 1.96.1 (31fca3adb), its `wasm32-unknown-unknown` standard library and wasm-tools 1.250.0. With those tools installed, rebuild from the independent Rust guest source.

```sh
cargo run --manifest-path examples/providers/guest/Cargo.toml --locked --offline --example build_guests
```
