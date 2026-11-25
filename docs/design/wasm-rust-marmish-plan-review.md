# WebAssembly, Rust and Marmish plan review

Claude Code CLI 2.1.332 reviewed all four plan documents in auto permission mode using the Sonnet model alias. The first pass requested revisions. It examined the plan against Oddlang's entry points and the pinned Marmish checkout. This record concerns design feasibility. Runtime evidence belongs to the implementation phases.

| First review concern | Disposition in the plan |
| --- | --- |
| Missing review artifact and unclear review provenance | This record documents the independent CLI review and dispositions. The work graph links it to the planning task. |
| Separate toolchains and offline dependency closure | The overview pins the current Rust baselines, isolates guest and proof toolchains and assigns offline preparation to the phase introducing each dependency. |
| Pure guest may acquire unexpected WASI imports | Phase 1 gates toolchain selection on inspecting and running a real component and specifies a fallback. |
| Guest independence and test-only exports | Phase 2 distinguishes the independent reference, generated guest, production world and instrumented state observations. |
| Narrow importer cannot parse real Marmish derives and error propagation | Phase 6 owns a versioned grammar extension and tests the recognized expansions and calls. Phase 3 diagnostics stay reproducible. |
| Missing project assembly interface and source provenance | Phase 6 adds a public assembly command and a negative test against copying the original source into a generated path. |
| Candidate ontology differs after regeneration | Phase 7 verifies each checkout against its own Git tree, with separate evidence for generated and retained paths. |
| Multi-platform evidence needs named runners | Phase 5 defines how target-specific evidence is obtained and how unavailable hosts appear in the report. |
| Structural diff could imply behavioral compatibility | Phase 1 tests the verdict vocabulary. Successful live pairings have separate evidence. |
| Claude review access and conflicting verdicts | The coordinator gives the reviewer a disposable checkout and resolves discrepancies using reproducible evidence and explicit acceptance criteria. |

Claude returned **APPROVE** on the revised plan after checking all ten dispositions. It suggested placing the project assembly interface before the codec task in the Phase 6 checklist. The checklist now follows that order.
