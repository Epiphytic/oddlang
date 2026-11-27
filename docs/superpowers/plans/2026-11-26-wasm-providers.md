# Replaceable providers implementation plan

Implement the [approved Wasm design](../specs/2026-11-26-wasm-provider-design.md) using Rust and versioned WIT components. Keep legacy manifests and locks reproducible. Inherited Marmish tools are outside the migration.

- [x] Implement generate and recover worlds in `contracts/providers` and the bounded component caller in `src/provider_runtime.rs`. Exercise actual components including traps, fuel exhaustion, excessive memory, wrong exports and forbidden imports.
- [x] Add explicit workspace registrations in `src/providers.rs`. Verify schema and component pins, contract version and configuration. Validate input documents and output paths, limits, dependencies and exports for both implementations. Test case-insensitive and file/directory collisions and atomic publication.
- [x] Route native public-record generation and source recovery through provider selection. Run independent consumers for both built-ins and replacements, a previously unknown capability and a selected library binding. Keep receipts separately versioned.
- [x] Inventory script ownership and caller references in [the baseline](../../../verification/provider-tooling/baseline.tsv). Port the four active Marmish command-policy tools, the Wasm matrix publisher and the public-record and public-constant qualifications. Preserve independent expectations.
- [ ] Port the remaining project-authored extraction, generation and verification scripts and switch their callers. Track the legacy migration separately from the qualified provider host.
- [x] Regenerate all seven crates from the 111 inputs, compare every emitted byte against the baseline, run 105 policy and 11 configuration cases on macOS ARM64, and retain the receipt. Run relevant compiler tests and validate the work ontology.
- [x] Install the four pinned OntoGPT skills, use the authoring guide to distinguish grounded identifiers from free text in provider schemas and verify RDF boolean lexical forms.
