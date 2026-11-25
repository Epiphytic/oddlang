# Coordinated ledger workspace

The project ontology imports the ledger by identity. The workspace lock selects its release. The test installs v1 and then replaces only that lock with the v2 pin. Rust, TypeScript, PostgreSQL and Avro projections all use the same verified release bytes.

Run the integration test from the repository root with the PostgreSQL 16.6 test database configured. Fetch the native dependencies and cache the TypeScript 5.9.3 compiler once before running offline.

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path examples/contracts/monorepo/native/Cargo.toml
npm exec --yes --package=typescript@5.9.3 -- tsc --version
cargo test --locked --offline --test monorepo
```

Each version builds a Rust service and CLI with checked account constructors and compiles the TypeScript client. The client reads the maximum signed 64-bit balance from the native account payload and checks both transfer outcomes against a running v1 executable. Transfers retain their v1 single-currency semantics. The v2 service and CLI carry currencyCode on current accounts. Historical rows and v1 Avro events acquire CAD through the named transition.

The test deletes generated sources and build output, rebuilds from the same pin offline, and compares the generated Rust, TypeScript, SQL, Avro, JSON, compiled JavaScript and native executables within the same build directory and toolchain.
