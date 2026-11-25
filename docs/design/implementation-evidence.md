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
