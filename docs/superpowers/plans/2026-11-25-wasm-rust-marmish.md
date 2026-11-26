# WebAssembly components, Rust round trips and Marmish qualification

**Goal.** Publish an executable WebAssembly contract, recover an editable Oddlang contract from existing Rust, and measure how much of a real upstream project can be rebuilt from its ontology with compatible behavior.

**Architecture.** WIT worlds give components explicit typed imports and exports. The Oddlang graph adds stable identity, invariants, behavior, and deterministic conformance obligations. The importer and emitter run locally in Rust with pinned inputs. Native consumers may implement a published contract independently. A component runtime is required only for a component deployment or its tests. [WIT reference](https://component-model.bytecodealliance.org/design/wit.html) · [Oddlang ledger semantics](../../language/ledger-core.md) · [Conformance bundle](../../language/conformance-bundle.md)

## Delivery order

Each row is an independently reviewed gate. The linked chapters specify fixtures, commands, ownership and acceptance checks. Follow the subagent-driven development workflow.

| Phase | Deliverable | Evidence needed to advance |
| --- | --- | --- |
| [1. Publish component contracts](wasm-component-support.md) | A locked, versioned WIT world projected from stable declarations | Deterministic publication and independent WIT parsing, invalid bindings rejected |
| [2. Run real components](wasm-component-support.md) | Compiled guest and host, with portable cases and a directional compatibility check | Live calls exercise both sides of the component boundary and detect a broken implementation |
| [3. Import and rebuild Rust](rust-round-trip.md) | A hand-written ledger operation imported into editable Turtle and emitted as compilable native Rust | Repeatable graph, explicit identity map, diagnostic for unsupported Rust and an optional cleanup workflow |
| [4. Validate behavioral equivalence](rust-round-trip.md) | Translation validation comparing original source with regenerated code | Independent oracle, differential tests and a scoped proof experiment that detects a deliberate regression |
| [5. Inventory Marmish](marmish-qualification.md) | An upstream baseline and a reviewed inventory covering the pinned workspace | Every owned target, contract and native boundary accounted for, including known qualification gaps |
| [6. Build missing Oddlang features](marmish-qualification.md) | Reusable importer, model, emitter and test extensions from the inventory | Upstream-derived fixtures and independent negative controls pass before application integration |
| [7. Qualify the reconstructed project](marmish-qualification.md) | A clean rebuild and differential exercise of the application, with a repeatable coverage report | A second checkout can reproduce the report and identify every regenerated, native and unsupported surface |

## What a compatibility claim covers

Record four independent facts for each claimed release, target and feature configuration.

1. **Reproducibility.** Identical pinned inputs produce identical canonical contract and projection bytes across clean builds. Record artifact digests and report binary reproducibility separately where the native toolchain introduces build paths, signing or other external inputs.
2. **Contract conformance.** Publish the obligation denominator and exact count of discharged obligations for each role. Gaps, skips, failed prerequisites and unsupported declarations retain their identities. Every declared case must pass for the claim “100% of declared obligations passed.”
3. **Behavioral equivalence.** Record the observable result, state and effects, input domain, proof assumptions and verifier outcome for each proven operation. A finite suite, an interface match or a timed-out proof supports a different claim. Ledger proofs do not transfer to another program.
4. **Regeneration coverage.** Inventory project-owned units and executable roots before selecting a pilot. Count graph-generated behavior, native or vendored implementations referenced by digest, and unsupported units separately. Report a hybrid rebuild even when all observed tests pass. A full application-source round trip additionally requires every in-scope project-owned implementation to regenerate from editable semantics, with zero opaque or unsupported units. Exclude third-party dependencies explicitly by package and pin. If gaps remain, publish them as named follow-up work with reproduction fixtures and repeat phases 6–7 before claiming full regeneration.

WIT extraction from an existing component can recover its interface. Exact recovery of an Oddlang-authored contract additionally requires its pinned ontology and provenance manifest. Rust source supplies behavior for the import experiment. Native binary recovery from a stripped executable is outside this plan.

## Toolchain and offline builds

| Working directory | Toolchain selection and responsibility |
| --- | --- |
| Oddlang root and Wasmtime host | Keep the existing HTTP baseline at Rust 1.96.1. Choose compatible pinned wasm-tools, WIT parsing, Wasmtime and Syn dependencies. Update the consuming Cargo manifests and their lockfiles in the phase that adds each dependency. A host dependency with a higher MSRV belongs in a separate pinned host package behind the runner command. |
| `examples/wasm/` | Phase 1 selects a guest compiler, target and component encoder by the pure-world execution spike and locks their exact versions, target libraries and checksums. Pin jco with its npm lock. Reuse the existing Node and Chromium pins. |
| `verification/rust-translation/` | Phase 4 locks Kani, its required nightly and backend independently. Use that toolchain only inside the proof package. Capture `rustc -Vv` for both the proof and the production build. |
| Original and rebuilt Marmish workspaces | Use upstream Rust 1.97.1, Cargo.lock, Git dependency revisions and each native platform. Keep build caches and outputs distinct from the root and proof packages. |

The setup scripts cache each Cargo registry/git closure using `cargo fetch --locked --manifest-path ...` for every workspace and target, install selected tool binaries and target libraries, and populate locked npm and Python caches. Invoke compilers by explicit toolchain or directory override. Check digests before offline verification and report a missing pin by name. Use an offline rerun in a fresh directory to test the setup itself.

## Execution and reviews

- The lead assigns each phase to a builder subagent and its acceptance fixtures to a separate test subagent. Give them disjoint files and agree on interfaces first. The lead integrates changes and owns the shared CLI, manifests and work graph where ownership overlaps. Separate the author of an expected value from the author of its generator.
- For every phase, start the work graph task, demonstrate its failing acceptance check, implement, run the command in its chapter, and save the tested commit and artifact digests. Preserve existing release bytes and run the affected prior gates. Run the complete existing and new offline suites before the final report. Setup may fetch pinned tools and fixtures in advance.
- After **each** integrated phase invoke Claude Code CLI with `claude -p --safe-mode --permission-mode auto` in a disposable checkout of the tested candidate, with a read-only review brief and writable scratch space for builds and tests. Supply the base and candidate Git SHAs, chapter, changed files, fixture locations and exact checks. Ask it to run independent commands and try to falsify the acceptance claims. Capture findings, evidence and dispositions under `docs/design/`. Fix material findings, rerun the affected checks, and get Claude to re-review the corrected candidate before advancing. Reproduce disputed findings against the written acceptance criterion. A passing test cannot waive a missing obligation, and a reviewer verdict cannot waive a failing check. Document counterevidence for a dismissed finding. If the reviewer is unavailable, retain the pending review on that phase.
- Keep the work graph and plan synchronized. Store stable phase identifiers, implementation evidence and review findings in `docs/design/work.ttl`. Open PRs against the relevant base branch. Use an isolated pinned checkout for Marmish and keep upstream acceptance fixtures independently runnable.
- [x] Phase 1 · component publication
- [x] Phase 2 · component execution
- [x] Phase 3 · Rust source import and regeneration
- [x] Phase 4 · translation validation
- [x] Phase 5 · Marmish baseline and inventory
- [x] Phase 6 · reusable features for Marmish
- [ ] Phase 7 · whole-project qualification

See the [plan review and dispositions](../../design/wasm-rust-marmish-plan-review.md).
