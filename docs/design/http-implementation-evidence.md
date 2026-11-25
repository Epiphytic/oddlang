# HTTP conformance implementation evidence

## Phase 1 · Vocabulary and offline loading

Claude Sonnet CLI reviewed `df5cfcf` in auto permission mode and passed both the specification and code-quality checks. It identified gaps in negative fixtures, semantic diagnostic ordering, setup version assertions and the precedence of legacy pin diagnostics. The follow-up checks those cases and makes the import path boundary explicit.

- Verified the editable-source and published-release CLI paths with the v1 fixture.
- Ran the pinned setup script, checked the downloaded OpenAPI schema digest, and launched the pinned Chromium build.
- Ran the existing workspace and release baseline offline with PostgreSQL 16.6. The database integration test also passed with its connection configured after Claude's unconfigured run reached that prerequisite.
- The scoped Claude follow-up on `7d4212a` found no blockers. Semantic validation names the first declaration and predicate in IRI order. Syntax and file-loading errors still use file order.
- `cargo fmt --all --check`, strict Clippy and the configured full offline suite passed. The new phase covers 15 HTTP model tests alongside the 45 existing ledger tests.
