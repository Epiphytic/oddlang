# Working on Oddlang

Oddlang explores whether a program can live in a hierarchy of ontologies. A domain model describes entities and invariants, a behavior model defines executable operations, and target overlays map stable concepts to native code, storage and wire formats. The long-term aim is to change one semantic model, query what depends on it, and deterministically rebuild the affected artifacts. The original Gemini conversation also explored portable ownership semantics, a concise authoring language and reviewed, locked LLM-assisted lowering. These are research directions. The first experiment is a two-version ledger written in Rust.

## Start here

- Read the [ledger design](docs/superpowers/specs/2026-11-25-ledger.md) for the current semantics, scope and success criteria. The original chat is available in the author's local PDF export named in the work graph. The design captures the agreed prototype for anyone working from a fresh clone.
- Follow the [implementation plan](docs/superpowers/plans/2026-11-25-ledger.md) for task details, fixtures and verification commands.
- Consult the [process vocabulary](docs/design/process.ttl) and [work graph](docs/design/work.ttl) for decisions, review findings, dependencies, progress and evidence. These describe work on Oddlang. The ledger's executable vocabulary has its own planned specification.

## What the prototype must demonstrate

1. **Structure and impact.** Generate a checked Rust account and a PostgreSQL table from the same domain model. Show that both enforce identical balance bounds. Query dependencies across local imports and compare the authoring and traversal cost with stable-ID TOML before investing further in RDF.
2. **Behavior.** Specify transfer semantics before building either executor. Compare the interpreter and compiled Rust with an independent ledger oracle on full state and named errors, including randomized boundary cases.
3. **Evolution.** Diff stable field identities, migrate populated v1 rows, and read saved v1 Avro events into checked v2 accounts. Exercise the rename, reader alias and historical CAD backfill. Verify a real Rust → PostgreSQL → Avro round trip.

Use Rust for the compiler, CLI, interpreter, generated execution target, tests and demo. Preserve deterministic bytes and diagnostics across equivalent graphs. Follow the plan's locked offline build and PostgreSQL setup when implementing the ledger.

## Maintain the design record

Use stable identifiers under `urn:oddlang:work:`. Keep the reusable classes and predicates in process.ttl and project facts in work.ttl. The plan owns detailed acceptance steps. The graph owns task relationships and status. Git preserves earlier revisions.

- For new work, add a named task with a label, milestone, source, acceptance condition and one status. Link prerequisites with `p:dependsOn`. New implementation tasks start as `p:planned`. Add the corresponding plan checkbox when the scope changes.
- When starting a task, set it to `p:active`. For blocked or deferred work, use `p:blocked` or `p:deferred` and record the reason. When verification succeeds, link an artifact containing the result with `p:evidence`, set `p:done` and check the plan item in the same change. A design revision proves a documentation change. Runtime claims require runtime evidence.
- Record a review concern as a finding with its source. Link each task that addresses it. Record design choices with rationale and source. When a decision changes, link the replacement to the earlier decision using `p:supersedes` and revise the human-readable design.
- Parse both Turtle files and run the [ready-work query](docs/design/ready.rq) after changing dependencies. It lists planned tasks whose transitive prerequisites are complete. With Apache Jena installed, run the following from the repository root.

```sh
arq --data docs/design/process.ttl --data docs/design/work.ttl --query docs/design/ready.rq
```

## Semble Code Search

Use `mcp__semble__search` to locate implementations by intent or name. Pass the repository root and choose `content: docs` for prose, `config` for configuration, or `all` for a mixed search. Navigate directly to the returned file and line and read the relevant definition before editing. Use `mcp__semble__find_related` with a file and line for similar code. Use grep only when every occurrence of a literal is needed.

The CLI fallback supports the same workflow. If the executable is unavailable, use `uvx --from "semble[mcp]" semble`.

```sh
semble search "load the typed domain model" . --max-snippet-lines 10
semble search "ledger compatibility criteria" . --content docs
semble find-related src/model.rs 42 .
```

## Finish a branch

After successful verification, push the feature branch and create a pull request against its base branch. Leave merging to the user. Present integration options only when push or PR creation is impossible or the user requests a different outcome.
