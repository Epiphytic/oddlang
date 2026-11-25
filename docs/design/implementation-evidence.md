# Ledger prototype implementation evidence

## PostgreSQL setup

- `cargo fetch --locked` and `cargo test --locked --offline --test database` passed against PostgreSQL 16.6 over loopback TCP. The server returned `160006`. Tests verified distinct schemas, cleanup after success and cleanup after an aborted transaction.
- Unset, malformed and unreachable test URLs produced setup diagnostics. The server version check rejected older and newer version numbers.
- Claude Code CLI reviewed the provisioning changes with `--permission-mode auto` and ran the database tests. The follow-up preserved safe connection error details, clarified when the demo lands and covered aborted-transaction cleanup.
- Docker Compose validated the configuration and reported `postgres:16.6`. The local container VM could not start. The live tests used a temporary loopback instance built from the PostgreSQL 16.6 source release.
