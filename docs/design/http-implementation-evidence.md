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

## Phase 4 · TypeScript and Go client adoption

Claude Sonnet CLI reviewed `721aa35` in auto permission mode. It found a detached browser Fetch receiver, inconsistent base URL and request-target handling, and missing peer bounds. The patch at `d60e179` fixes those behaviours and tests actual Chromium and the matching Go prefix resolution. It also aligns the provider and grader with the clients' handling of quoted UTF-8 charset values.

- Each client passed all 193 pinned consumer cases over independently scripted native HTTP peers. The shared verifier accepted each report. Each client also retrieved the exact balance 9007199254740993 from the live Rust provider.
- Each encoder mutant failed exactly two cases. Each decoder mutant failed exactly 28 cases. Unaffected observations still passed. The native runners rejected altered payload and manifest bytes.
- The standalone Go fixture ran from a temporary directory with module downloads disabled. The TypeScript client imports no Node APIs and uses a bounded Node test runner and a real Chromium test. The provider, consumer and bundle suites all passed after review fixes.
- The scoped Claude follow-up approved `d60e179` with no remaining blockers.

## Phase 5 · Version skew and expanded JSON codecs

Claude Sonnet CLI reviewed the wire-evolution commit `b1bed28` in auto permission mode and passed both gates. It noted that the ledger scenario derivation relies on a stable balance declaration. The separate codec fixture has its own portable rules and checks.

- The v2 manifest SHA-256 is `0922d46293a8bb9f8a5221dffa1fd5a87850299efdc99efe14ad659163276b0e`. The published compatibility report names the unchanged v1 digest and the renamed field IRI. Unchanged authors' behavior fixtures project through the new wire name. Native Rust and TypeScript names remain stable.
- Same-version live lookup pairs pass. Both mixed pairs return an HTTP 200 payload that fails the client's expected field decode. A mixed transfer returns a client protocol error and independent reads confirm the provider committed balances A=7 and B=10.
- All 117 v2 provider cases and all 193 v2 TypeScript cases passed. The external Go v1 target lock reproduced its 193 cases and rejected a changed native source file. V1 republication preserved every pinned byte and accepted the saved provider and client evidence.
- Impact selection walks reverse dependencies. A changed named ledger balance constraint affects reader, writer and authored operation checks. An unrelated IRI selects none. Multiple contract identities resolve together and conflicting pins fail.
- Claude Sonnet CLI reviewed the separate codec commit `797cedd` in auto permission mode. It built all three codecs and compared additional raw wire inputs. The follow-up adds portable raw-byte witnesses and a regression proving that the shared model requires a closed record. Rust, TypeScript and Go each pass the same 82-case corpus with 40 obligation rows. The manifest SHA-256 is `867e93132a34eb3d446b7faef2cf36af89fd25767abaded84975c9adbe5fbff3`. The separate `json.codec` role checks nested records, lists, optional and required nullable fields, and closed tagged unions independently of transport. Both original ledger HTTP bundle digests are unchanged.

## Phase 6 · Node SSR and React against live Rust

Claude Sonnet CLI reviewed `9b50441` in auto permission mode. It verified the two-version hybrid flow by inspection and requested explicit runtime, role, receipt and scope documentation plus prerequisite pin checks. Commit `933bd3d` shares prerequisite checks between setup and offline verification, requires fresh receipts, and verifies Go's evidence using its independently owned target pin. Claude Opus CLI also reviewed the whole branch in auto mode and found no release-blocking cross-phase issues.

- The same Node TypeScript SSR and React sources build with each workspace pin. Node renders the opening balances from the live Rust provider. Chromium submits a transfer of 3, observes the release-specific wire field, renders balances 7/10 and sees the named insufficient-funds error on a transfer of 8. Independent provider reads confirm the failed transfer leaves balances unchanged.
- The v1 Rust provider remains live for independently pinned Go while the workspace exercises v2. Both retained deployments under `target/http/v1` and `target/http/v2` also started and served the pages from their built artifacts.
- Claude's scoped auto-mode follow-up approved both the codec and hybrid revisions. The full pinned offline gate passed 85 tests with PostgreSQL 16.6, and the CLI verified eight fresh native evidence files. Strict Clippy, Rust formatting, the Turtle files and the ready-work query also passed.
