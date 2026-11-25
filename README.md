# Oddlang

Oddlang explores compiling a ledger's domain, behavior and target mappings
from versioned ontologies. See the [ledger design](docs/superpowers/specs/2026-11-25-ledger.md)
and [implementation plan](docs/superpowers/plans/2026-11-25-ledger.md).

## HTTP conformance

Publish a pinned HTTP contract with portable test cases, OpenAPI and JSON
Schema. Generate a native Rust provider and a shared browser-safe TypeScript
client. A standalone Go client adopts the same cases with its own release pin.
The [hybrid example](examples/http/hybrid/README.md) shows setup, offline
verification and a Rust backend with Node SSR and React.

## WebAssembly component contracts

Publish the ledger world and recover its interface into editable Turtle with the release's stable identity map.

```sh
cargo run --locked --offline -- wasm publish contracts/wasm/ledger contracts/wasm/empty.lock.json contracts/cache target/wasm-ledger
cargo run --locked --offline -- wasm import-wit target/wasm-ledger/wit ledger target/wasm-ledger/identities.json target/wasm-recovered
```

Each publication includes WIT, a manifest, a workspace lock and conformance cases. Pin the workspace lock independently when consuming a release. A WIT import recovers the interface. The release ontology carries additional behavior and constraints.

Run `scripts/setup-wasm.sh` once with network access. Then run `scripts/verify-wasm-spike.sh` to compile the pure echo guest, inspect its imports and call it from an empty Wasmtime linker offline.

Run `scripts/verify-wasm.sh` for ledger execution. It publishes fresh bundles, builds handwritten and generated Rust components in two directories, and runs portable cases against Wasmtime, Node and headless Chromium. Its output directory holds release pins, Rust source, component binaries, host observations and a build digest comparison. Ledger guests use `wasm32-unknown-unknown` with Component Model bindings. The integer echo uses `wasm32-wasip2`.

## Integration tests

The full suite uses a Rust toolchain with edition 2024 support, PostgreSQL 16.6, Node.js and npm for the TypeScript client, and Go 1.22 or newer for the independent client. The client builds were exercised with Node.js 22 and npm 10. While online, cache the database image, both locked Rust dependency sets and the pinned TypeScript compiler.

```sh
docker compose pull
cargo fetch --locked
cargo fetch --locked --manifest-path examples/contracts/monorepo/native/Cargo.toml
npm exec --yes --package=typescript@5.9.3 -- tsc --version
```

Start the disposable database, set its connection URL and run the suite
offline.

```sh
docker compose up -d --wait
export ODDLANG_TEST_DATABASE_URL=postgresql://oddlang:oddlang@127.0.0.1:55436/oddlang
cargo test --locked --offline
```

Compose waits for the `pg_isready` TCP health check. The test harness also
connects over TCP and verifies `server_version_num = 160006` (PostgreSQL 16.6)
before creating an isolated schema for each test. An existing PostgreSQL
16.6 instance can be used by setting the URL to a disposable database
whose user may create schemas. The ledger demo uses the same URL.

When finished with the Compose database, remove its containers and data
with `docker compose down -v`.

## Contract tests

Run the contract checks separately with the same offline caches. The monorepo test uses the configured PostgreSQL database.

```sh
cargo test --locked --offline --test contract_release
cargo test --locked --offline --test monorepo
cargo test --locked --offline --test adoption
```

The [monorepo fixture](examples/contracts/monorepo/README.md) changes one workspace pin, builds the Rust and TypeScript consumers, and verifies the SQL migration and Avro reader. The [standalone Go client](examples/contracts/go-client/README.md) adopts the releases with its own pins and replays a digest-checked adapter.

## Ledger demo

Run the demo from the repository root after starting PostgreSQL 16.6.

```sh
cargo run --locked --offline --example ledger_demo
```

The demo builds both versions, queries balance dependencies, executes transfers,
migrates populated accounts and reads a saved v1 Avro event with the v2 reader.
The v2 root imports `v1-to-v2.ttl`, whose historical CAD value supplies both
the SQL backfill and Avro reader default. New SQL rows and generated Rust
accounts require an explicit currency. The v2 model defines no transfer
operation. To keep the generated artifacts, run
`cargo run --locked --offline -- build examples/ledger/v2 target/ledger-v2`.

## Recover a Rust ledger contract

The Rust source importer accepts the [ledger-v1 grammar](docs/language/rust-import.md). It validates an independent Cargo library, produces editable Turtle and records the source, mapping, toolchain and graph digests. Run these commands from the repository root with the pinned Rust toolchain.

```sh
cargo run --locked --offline -- import rust examples/rust-import/ledger/Cargo.toml examples/rust-import/ledger/mapping.toml target/import-ledger
cargo run --locked --offline -- build target/import-ledger target/import-ledger-native
cargo run --locked --offline -- import replay examples/rust-import/ledger/Cargo.toml examples/rust-import/ledger/mapping.toml target/import-ledger
cargo run --locked --offline -- import verify-build target/import-ledger target/import-ledger-native
```

Replay compares the current inputs and regenerated artifacts with the saved import. To develop a new contract revision, copy the Turtle closure into a new project directory and use the normal build command. The recovered project and its import lock serve as the original translation record.
