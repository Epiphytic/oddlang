# Select a Rust source API binding for recovered package components

**Goal.** Allow a recovered record component to use a separately pinned Rust role assertion while preserving its immutable recovery graph and binding.

- [x] Compare native artifacts and role provenance for a passive record and enum. Exercise wrong role, changed declarations, conflicting assertions, and stale pins.
- [x] Reuse the public-records Rust role check and preserve legacy builds.
- [x] Qualify the #126 package, record verification, and review the framework change.

A compact recovered public record can specify a `sourceApi` file pin. The pin names an existing public-records Rust overlay with a `sourceApi` role binding. Its non-role values must match the immutable recovered binding. Projection checks the selected role with the existing typed field requirements. Both immutable recovery input pins and the separate one are retained in the package lock.

The [macOS ARM64 receipt](../../../verification/recovered-public-data/receipts/source-api-aarch64-apple-darwin.json) records a downstream typed caller, seven rejections, and byte equality for 11 legacy artifacts. Run `python3 -B verification/recovered-public-data/source_api.py PATH_TO_ODDLANG` to reproduce the generic fixture. A compact record that already embeds a role binding rejects a second assertion. An existing compact or explicit component can continue to omit the new field.
