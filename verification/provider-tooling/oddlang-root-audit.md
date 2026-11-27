# Oddlang tooling and consumer audit

The Oddlang rows in `baseline.tsv` at revision `6f9e8475cc4d24ffe67d73c887f3a21e00f21d07` list 133 Python files, 19 shell scripts, four TypeScript files and six ES modules. Comparing those paths with `git ls-tree -r fc30767699e50dbc3bf3a2a5956e97b52ae342a3` leaves all ten JavaScript/TypeScript paths and none of the 152 Python/shell paths. The local command client and local owner workflow have separate verification receipts. This audit covers the other Oddlang families.

The audited tree has 28 files across Python, CJS, MJS, TS, TSX and Go. There are no other tracked shell, JS, Ruby, PowerShell or Perl scripts and no executable-mode files. Checking interpreter shebangs also finds `tests/fixtures/rust-tree-control/src/bin/shebang.rs`, a Rust parser fixture. The four Git symlinks point to the upstream OntoGPT skill directories. None of the six tracked files named `package.json` defines a scripts command. The TypeScript fragments in Rust projections are generated consumer support, including `examples/contracts/monorepo/transfer.ts.inc`.

## Callers and roles

Within each directory below, the filenames form the complete list for the audited extensions. Callers include both direct invocations and the Rust entry that builds or exercises a generated consumer.

| Directory | Files | Caller and responsibility |
| --- | --- | --- |
| `examples/contracts/go-client` | `client_test.go`, `lock_test.go`, `revisions/ledger-2/client.go`, `tools/replay.go` | `tests/adoption.rs` runs the standalone Go consumer. Its documented Go-only replay entry checks its own pins and runs native Go tests in isolation. |
| `examples/contracts/monorepo` | `wire.ts`, `verify.cjs` | `project.rs` generates a TS import of the wire helpers. `tests/monorepo.rs` invokes the JS consumer, which decodes actual TS BigInts and sends requests to the built Rust participants. |
| `examples/http/clients/go` | `client.go`, `peer_test.go` | `tests/http_consumers.rs` and `tests/http_hybrid.rs` run the independent Go HTTP peer. |
| `examples/http/clients/typescript` | `generated/client.ts`, `peer.mjs` | `tests/http_consumers.rs` builds the generated client and invokes the Node Fetch peer. |
| `examples/http/contract/shapes/v1` | `go-runner/main.go`, `typescript-runner.cjs` | `tests/http_shapes.rs` exercises the independent Go and TypeScript shape consumers. |
| `examples/http/hybrid` | `ssr/server.ts`, `web/main.tsx` | `tests/http_hybrid.rs` builds the Node SSR and React browser applications. |
| `examples/marmish/portable-oracle` | `tagged_adapter.mjs` | `verification/marmish-inventory/src/bin/qualify-tagged` builds and invokes the adapter for the generated TS codec. |
| `examples/wasm/javascript` | `host.ts`, `run.mjs` | `wasm-js-hosts` transpiles and compiles the bindings. The JS adapter performs native Node WebAssembly compilation, serves generated modules on loopback, runs Chromium streaming compilation and returns both runtime transcripts. |
| `tests/support` | `http-browser-fetch.cjs`, `http-hybrid.cjs`, `http-schema.cjs`, `http-skew.cjs` | `tests/http_consumers.rs`, `http_hybrid.rs`, `http_bundle.rs` and `http_evolution.rs` use Chromium Fetch, React/DOM interaction, pinned Ajv and actual TS client version skew, respectively. |
| `tests/support` | `tagged-wire-stream.mjs`, `tagged-wire.mjs` | `tests/tagged_wire_typescript.rs` invokes Node on generated codecs, checking incremental and complete frame observations. |
| `verification/checked-values` | `typescript.mjs` | `tests/checked_values_typescript.rs` exercises the generated TS checked-value API. |
| `verification/provider-validation/fixtures/choices` | `negative.ts`, `observer.ts` | `qualify-choices/typescript.rs` invokes the TS compiler on negative fixtures and runs the generated consumer. |
| `skills` | `ontogpt-author-template/scripts/validate_template.py`, `ontogpt-select-template/scripts/describe_template.py` | Invoked by the two upstream skill guides. Distribution provenance and unchanged upstream directories are recorded in `ontogpt-skills.json`. |

## Runtime verification

The Rust Wasm qualifier now checks the workspace and corpus hashes, derives expected calls and imports, hashes the component and binding files, validates the actual host versions and writes both host receipts. A live replay on Apple Silicon used Node v22.22.2, Chromium 145.0.7632.6, jco 1.42.0 and TypeScript 5.9.3. Each of the manual and generated components passed four ledger and five ledger-test scenarios in both hosts, giving 36 case observations. The eight parsed receipts matched the reports pinned in `wasm-verify.json` on every field except the SHA-256 of the edited host driver. The frozen reports retain their historical digests. Mutating the manifest bytes, mutating the corpus bytes or resealing a deliberately wrong expected call result each produced exit 1 at the corresponding Rust check.

The Rust hybrid integration test now starts the SSR process, reads its loopback address with a five-second deadline and reaps it on scope exit. The JavaScript consumer still checks the SSR response, drives a real browser transfer, checks the named insufficient-funds error and reads back both balances. A fresh build of `tests/http_hybrid.rs` using the qualified Oddlang CLI passed its single integration test, including both v1 and v2 and the final independent Go lookup.
