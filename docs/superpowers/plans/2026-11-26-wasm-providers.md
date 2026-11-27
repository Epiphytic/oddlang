# Replaceable providers implementation plan

Implement the [approved Wasm design](../specs/2026-11-26-wasm-provider-design.md) using Rust and versioned WIT components. Keep legacy manifests and locks reproducible. Inherited Marmish tools are outside the migration.

- [x] Implement generate and recover worlds in `contracts/providers` and the bounded component caller in `src/provider_runtime.rs`. Exercise actual components including traps, fuel exhaustion, excessive memory, wrong exports and forbidden imports.
- [x] Add explicit workspace registrations in `src/providers.rs`. Verify schema and component pins, contract version and configuration. Validate input documents and output paths, limits, dependencies and exports for both implementations. Test case-insensitive and file/directory collisions and atomic publication.
- [x] Route native public-record generation and source recovery through provider selection. Run independent consumers for both built-ins and replacements, a previously unknown capability and a selected library binding. Keep receipts separately versioned.
- [x] Inventory script ownership and caller references in [the baseline](../../../verification/provider-tooling/baseline.tsv). Port the four active Marmish command-policy tools, the Wasm matrix publisher and the public-record and public-constant qualifications. Preserve independent expectations.
- [ ] Port the remaining project-authored extraction, generation and verification scripts and switch their callers. Track the legacy migration separately from the qualified provider host.
- [x] Regenerate all seven crates from the 111 inputs, compare every emitted byte against the baseline, run 105 policy and 11 configuration cases on macOS ARM64, and retain the receipt. Run relevant compiler tests and validate the work ontology.
- [x] Install the four pinned OntoGPT skills, use the authoring guide to distinguish grounded identifiers from free text in provider schemas and verify RDF boolean lexical forms.
- [x] Migrate import cleanup, JS host replay orchestration, Wasm report generation and independent checks for native resource limits, field relations, checked values, choices, JSON input, file access and public-root data. Compare accepted archives and host receipts byte-for-byte and rerun the Marmish gate after extending the CLI.
- [x] Compare the Rust assembly preparers against the historical v1 and v2 content stores and manifests. Reproduce Rust declaration and native consumer observations through the Rust verification tools.
- [x] Replay the published framing build-closure archive with the Rust verifier, repeat its sandbox denial control, compare all compiler prerequisites and package pins, and check the ordered build-report and recovery fixtures.
- [x] Replay native package projection, sibling workspace linking, cold and disconnected Git acquisition, and both native process wrappers with independent Rust callers and fresh-output rejection controls.
- [x] Rebuild the pinned Marmish source inventory in Rust, compare its archive and summary byte-for-byte, and verify lexical scope, platform eligibility, packaging recipes and open completeness obligations.
- [x] Replay the archived Phase 7 source trees, deterministic commit, framing evidence, mixed wire and native observations with the Rust orchestrator. Verify its aggregate bytes and nine negative controls.
- [x] Recover the original native evidence and recompute the native candidate comparison in Rust. Compare all archived report bytes, exercise transcript and receipt mutations, and verify the candidate Git trees before issuing a new comparator receipt.
- [x] Port the native baseline collector and candidate runner integration to Rust. Compare all 22 command identities with the archive, test sandbox denial and process-group timeouts, and run the pinned ontology, Cargo metadata and core contract tests.
- [x] Qualify the native stream driver against the original and candidate workspaces, replay both local wire directions and the discriminating encoder mutation, and compare the complete original receipt with its historical runner.
- [x] Collect original and regenerated stream package compiler inputs in Rust, compare all three build modes and 29 native test outcomes with their archived receipts, and exercise the six declaration and dependency controls.
- [x] Port the source-free stream workspace and graph-mutation runners, regenerate all five Rust files, rerun both compiler controls, the named downstream consumer observations, and the 50-case mutation oracle.
- [x] Migrate HTTP setup, version and schema pins, and offline qualification to Rust. Verify fresh provider, client and codec receipts and both hybrid rendering targets.
- [x] Replace the recovery profile shell launcher with the native capture executable, reproduce the original stream structure and rustdoc differential, and qualify compiler, shape, and export witnesses.

- [x] Replace the retained-room qualification launcher with Rust and replay its independent local authorization and commit/reopen assertions. Compare the complete generated workspace before and after the provider refactor. [macOS ARM64 evidence](../../../verification/provider-tooling/retained-room-fixture-rust.json)

- [x] Route workspace closure, external package comparison, offline Git cache and original library test preparation through Rust. Compare the staged bytes for every published workspace manifest, rebuild the profile command-policy product, and run the original core and stream suites. [macOS ARM64 evidence](../../../verification/provider-tooling/workspace-inputs-rust.json)

- [x] Replace the v5 cold-cache Python verifier with Rust, including empty-cache refusal, locked acquisition, offline package and source verification, release compilation and transcript comparison for both executables. Check the historical and unified product layouts through the shared Rust verifier. [macOS ARM64 evidence](../../../verification/provider-tooling/cold-workspace-rust.json)

- [x] Recover the linked MissingRange record from Rustdoc and stage the native package inputs with Rust. Verify both archived source variants, scope denials, stale-selector rejection and native caller behavior with 35 cases per variant. Reject a changed recovered graph before publication. [macOS ARM64 evidence](../../../verification/provider-tooling/native-range-recovery.json)

- [x] Move Rustdoc provenance validation into the range preparer. Replay historical v1 and freshly collected v2 inputs through the pinned qualification, compare the post-refactor projection on the fresh documents, and verify rejection of changed pins, private-item settings, crate identity and duplicated documents. [Both macOS ARM64 runs](../../../verification/provider-tooling/native-range-recovery.json)
