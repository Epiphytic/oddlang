# Oddlang

Oddlang explores compiling a ledger's domain, behavior and target mappings
from versioned ontologies. See the [ledger design](docs/superpowers/specs/2026-11-25-ledger.md)
and [implementation plan](docs/superpowers/plans/2026-11-25-ledger.md).

## Integration tests

While online, cache the pinned PostgreSQL image and locked Rust dependencies.

```sh
docker compose pull
cargo fetch --locked
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
whose user may create schemas. The planned ledger demo will use the same URL.

When finished with the Compose database, remove its containers and data
with `docker compose down -v`.
