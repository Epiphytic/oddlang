# Coordinated ledger workspace

The project ontology imports the ledger by identity. The workspace lock selects its release. The test installs v1 and then replaces only that lock with the v2 pin. Rust, TypeScript, PostgreSQL and Avro projections all use the same verified release bytes.

Run the integration test from the repository root with the PostgreSQL 16.6 test database configured. Fetch the native dependencies and cache the TypeScript 5.9.3 compiler once before running offline.

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path examples/contracts/monorepo/native/Cargo.toml
npm exec --yes --package=typescript@5.9.3 -- tsc --version
cargo test --locked --offline --test monorepo
```

Each version builds a Rust service and CLI with checked account constructors and compiles the TypeScript client. Each client reads the maximum signed 64-bit balance from its native account payload. The v1 client sends requests to the v1 service and checks successful balances and the named backend error. The v2 release defines account structure only. Its client, service and CLI decode the required nonempty currencyCode on current accounts. Historical rows and v1 Avro events acquire CAD through the named transition.

The test deletes generated sources and build output, rebuilds from the same pin offline, and compares the generated Rust, TypeScript, SQL, Avro, JSON, compiled JavaScript and native executables within the same build directory and toolchain.
