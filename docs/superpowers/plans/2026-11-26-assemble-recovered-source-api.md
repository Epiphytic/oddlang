# Select a Rust source API binding for recovered package components

**Goal.** Allow a recovered record component to use a separately pinned Rust role assertion while preserving its immutable recovery graph and binding.

- [ ] Compare native artifacts and role provenance for a passive record and enum. Exercise wrong role, changed declarations, conflicting assertions, and stale pins.
- [ ] Reuse the public-records Rust role check and preserve legacy builds.
- [ ] Qualify the #126 package, record verification, and review the framework change.

A compact recovered public record can specify a `sourceApi` file pin. The pin names an existing public-records Rust overlay with a `sourceApi` role binding. Its non-role values must match the immutable recovered binding. Projection checks the selected role with the existing typed field requirements. Both immutable recovery input pins and the separate one are retained in the package lock.
