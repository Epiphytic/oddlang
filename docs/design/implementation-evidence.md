# Ledger prototype implementation evidence

## PostgreSQL setup

- `cargo fetch --locked` and `cargo test --locked --offline --test database` passed against PostgreSQL 16.6 over loopback TCP. The server returned `160006`. Tests verified distinct schemas, cleanup after success and cleanup after an aborted transaction.
- Unset, malformed and unreachable test URLs produced setup diagnostics. The server version check rejected older and newer version numbers.
- Claude Code CLI reviewed the provisioning changes with `--permission-mode auto` and ran the database tests. The follow-up preserved safe connection error details, clarified when the demo lands and covered aborted-transaction cleanup.
- Docker Compose validated the configuration and reported `postgres:16.6`. The local container VM could not start. The live tests used a temporary loopback instance built from the PostgreSQL 16.6 source release.

## Milestone 1

- `cargo test --locked --offline` passed 13 tests against PostgreSQL 16.6 after review fixes. A separately compiled Rust output and live SQL agreed at -1, 0, i64::MAX and i64::MAX + 1. The SQL test distinguished a CHECK violation from BIGINT overflow and exercised a narrowed upper CHECK.
- `oddlang check`, `build` and `impact` ran against the v1 example. Tests verified byte-identical Rust and SQL output under triple reorder, prefix change and blank-node relabeling. The [impact comparison](impact-comparison.md) records the exact query result, import topology and criterion for retaining Turtle.
- Claude Code CLI reviewed the milestone in auto mode, ran the offline suite and tested mutated inputs. Review fixes rejected Rust names that cannot compile, invalid graph ownership and dependency edges, and unknown impact IRIs. Tests now verify the precise SQL and typed-loader diagnostics.
- The complete suite passed after the fixes. Formatting and Clippy with warnings denied also passed. The live setup rejected a separate PostgreSQL 14 server. The ARM64 Compose image was cached as an OCI layout and its manifest digest and 14 layer hashes matched the published image.

## Language contract

- The closed vocabulary and evaluation rules are documented in the [ledger core contract](../language/ledger-core.md). A hand-authored fixture pins the expanded IRIs and operand order. Mutation tests verify rejected cycles, invalid operands, unguarded reads, error identities and stable named-owner diagnostics.
- Claude Code CLI ran the offline tests and reviewed the language contract in auto mode. Its review found repeated lowering of shared expressions and manually duplicated impact edges. The prototype now bounds expression depth and expanded nodes, and SPARQL derives the affected fields directly from Read and Assign operands.
- All 21 tests passed offline against PostgreSQL 16.6 after the review fixes, with formatting and strict Clippy clean. The independent oracle was prepared during this review because it consumes the design table directly. Executor work began after the language review fixes passed.

## Milestone 2

- The independent oracle, interpreter and separately compiled Rust matched on 12,000 seeded cases, including every guard error and complete state on every call. The run included 1,144 successful transfers. The corpus samples amounts at and around zero, source funds and destination headroom.
- Claude Code CLI reviewed the transfer phase in auto mode. It ran the full locked offline suite against PostgreSQL 16.6, tried five alternate seeds, and verified that intentional changes to the interpreter, emitter and shared lowering caused the oracle comparison to fail.
- All eight transfer tests passed after review fixes. Two swapped assignment operands now produce an agreed success with the wrong state, which the oracle catches. The test harness now shrinks coupled boundary cases and isolates compiled crashes to a replayable case. It also exercises two distinct missing IDs. The full locked offline suite passed after the fixes.

## Milestone 3

- A generated v1 Rust account round-tripped through a live PostgreSQL row and binary Avro with all fields intact. A raw negative-balance event was rejected by the checked constructor with the balance IRI.
- The named v1-to-v2 transition supplied the historical CAD value. PostgreSQL migrated populated rows in one transaction and produced the same catalog as a fresh v2 build. An omitted currency failed, an explicit JPY survived SQL and Avro, and rollback preserved v1 after an injected mid-migration failure.
- The IRI-keyed compatibility verdict and an independent Arrow Avro decode each rejected the on-disk variants missing only the owner alias or only the currency reader default. The saved v1 event matched the migrated v2 row in every field. The confirmed direction is v1 writer to v2 reader. The pinned Arrow reader also accepts a writer-side alias in the reverse direction, which the prototype's directional verdict does not count as reader-side compatibility.
- Claude Code CLI reviewed Milestone 3 in auto mode and independently ran the locked offline suite and live demo against PostgreSQL 16.6. After review fixes, the 21 scoped model, target and evolution tests passed. The demo reported the graph experiment separately, showed a successful transfer and both competing-failure cases, and named the missing reader default. Avro schemas now participate in the equivalent-graph byte check and a v2 schema fixture pins the alias and default.

## Contract interoperability

- Both release manifests pin the complete Turtle import closure, JSON wire rules, portable vectors and directional compatibility notes. A single workspace lock pins the manifest digest. The offline resolver loads verified bytes, detects absent or altered files, and rejects incoherent metadata or unreachable ontology files even when the digests are recomputed.
- The coordinated monorepo test replaces one workspace pin while keeping the importing ontology identical. It builds the native Rust service and CLI, compiles the generated TypeScript client, and ties the PostgreSQL and Avro artifacts to the populated migration and saved-event checks. Both versions preserve the maximum balance as decimal text. The v1 service and client verify transfer success and a named backend error. The v2 release and service expose the upgraded account contract. Both builds reproduce the generated source, schemas, Rust binaries and JavaScript bytes offline under the same paths and toolchain.
- The standalone Go client runs with only its source, published bundle and its own pins. It validates the request and response wire rules for the v1 transfer, stays pinned when the publisher upgrades, then verifies the v2 account contract after an explicit upgrade. The target lock selects the recoverable adapter revision and pins its source, contract, overlay and vectors. Modified or extra adapter source is rejected before compilation.
- Claude Code CLI reviewed interoperability and the whole branch in auto mode, then ran a scoped re-review of the fixes. The follow-up confirmed 16 focused tests and found no unresolved material defects. The final offline suite passed all 45 tests with PostgreSQL 16.6. The live demo, root and native fixture formatting, strict all-target Clippy and full-branch whitespace checks passed.
- The prototype resolves one published release for a project. The release binds JSON names in its wire metadata and field identities in its ontology. Independent consumers supply their own role-specific native code.
