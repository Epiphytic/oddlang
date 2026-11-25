# Rust, Node SSR and React ledger

The Rust provider runs the pinned ledger behavior. A TypeScript server renders
account balances in the initial HTML. A browser React app uses the same
generated Fetch client to read accounts and submit transfers. The Node server
proxies the browser's API requests to the local Rust provider.

## Verify from a fresh checkout

Run the setup command online to cache the locked toolchains and dependencies.
Configure the PostgreSQL 16.6 test connection described in the
[repository README](../../../README.md) for the existing ledger suite.

```bash
scripts/setup-http-conformance.sh
scripts/verify-http-conformance.sh
```

The offline verification runs the existing ledger tests and the HTTP provider,
consumer, release, codec and hybrid checks. It verifies complete provider and
consumer evidence for the published claims under `target/http-conformance/`.
The pinned Go v1 client also runs from a temporary copy with module downloads
disabled.

The hybrid test builds the same application source twice. It keeps the import
by contract identity and switches the workspace lock from v1 to v2. Both apps
continue using the native `balance` property. Chromium checks `balance` on
the v1 wire and `balanceMinor` on the v2 wire. It runs a transfer of 3, then
observes the named insufficient-funds error for a transfer of 8 and verifies
the resulting balances directly with the provider.

## Run the built demo

The verification command leaves the built programs under `target/http/v1`
and `target/http/v2`. Start the v1 provider and frontend in two terminals
from the repository root.

```bash
target/http/v1/provider --fixture target/http/v1/fixture.json --listen 127.0.0.1:32101
```

```bash
ODDLANG_PROVIDER_URL=http://127.0.0.1:32101 ODDLANG_HYBRID_PORT=32102 \
  node target/http/v1/dist/ssr/server.js
```

Open http://127.0.0.1:32102/ for server-rendered balances. The Transfer funds
link opens the browser React view.

The demo covers the finite HTTP contract profile and both ledger wire
releases. It uses loopback listeners and fresh in-memory fixtures.
