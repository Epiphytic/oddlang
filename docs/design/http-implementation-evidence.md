# HTTP conformance implementation evidence

## Phase 1 · Vocabulary and offline loading

Claude Sonnet CLI reviewed `df5cfcf` in auto permission mode and passed both the specification and code-quality checks. It identified gaps in negative fixtures, semantic diagnostic ordering, setup version assertions and the precedence of legacy pin diagnostics. The follow-up checks those cases and makes the import path boundary explicit.

- Verified the editable-source and published-release CLI paths with the v1 fixture.
- Ran the pinned setup script, checked the downloaded OpenAPI schema digest, and launched the pinned Chromium build.
- Ran the existing workspace and release baseline offline with PostgreSQL 16.6. The database integration test also passed with its connection configured after Claude's unconfigured run reached that prerequisite.
- The scoped Claude follow-up on `7d4212a` found no blockers. Semantic validation names the first declaration and predicate in IRI order. Syntax and file-loading errors still use file order.
- `cargo fmt --all --check`, strict Clippy and the configured full offline suite passed. The new phase covers 15 HTTP model tests alongside the 45 existing ledger tests.

## Phase 2 · Published obligations, cases and schemas

Claude Sonnet CLI reviewed `61d114d` in auto permission mode. It found a missing provider obligation for empty and dot-segment path IDs. The fix at `113d12e` adds all three witnesses, malformed-decimal cases, unsupported media parameters and a media-type precedence case. It also adds the checked-in release replay test and uses correctly escaped local schema references.

- The v1 manifest SHA-256 is `e1cf1857145ff54c538de9f14938fae607e1e925a5220dcc9feafb92aa60266a`. There are 134 obligation rows, 117 provider cases and 193 consumer cases.
- Nine bundle tests and 15 model tests passed offline. Tests exercise independent ledger outcomes and full post-state, witness deletion, invalid or unexecuted evidence, normalization and byte-identical publication.
- Ajv 8.20.0 checks JSON Schema and the pinned OpenAPI metaschema with its dynamic references resolved to the single effective anchor. A separate validator checked the unchanged OpenAPI metaschema. The release gate uses atomic staged writes through the existing `tempfile` crate.
- The scoped follow-up found no remaining blockers. The release verifier checks that authored cases refer to valid witness slots. The ledger fixtures also have independent oracle checks for their semantics. New authored scenarios still require equivalent independent review.
- A clean source snapshot of the reviewed Phase 2 code passed all 71 offline tests with PostgreSQL 16.6 configured.

## Phase 3 · Live Rust provider

Claude Sonnet CLI reviewed `3ca48e3` in auto permission mode, passed the spec and quality gates, and probed the generated provider on loopback. It flagged the unbounded request read. The follow-up caps the example's buffered JSON request body at 1 MiB and checks the boundary against the live provider. Unmatched routes and method errors remain outside the declared HTTP operations.

- The provider uses pinned Axum 0.8.9 and Tokio 1.54.1 plus generated native Rust ledger behavior. Regeneration reproduces the checked-in provider source. Advisory overlays did not change the generated binding.
- The stock provider passed all 117 released provider cases over 378 HTTP exchanges, with a fresh process and fixture state per case.
- Three test-only providers failed the expected observations for write-on-error, changed guard order and noncanonical amount handling. The stock provider evidence passed standalone CLI verification.
- The scoped Claude follow-up on `a1fd6fd` approved the body cap. The live regression failed before the fix with 200 and passed afterward with 400, alongside all provider checks.
