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

The pinned runtimes are Rust 1.96.1, Node 22.22.2, npm 10.9.7 and Go 1.26.3.
The browser test uses Playwright 1.58.2 and Chromium 145.0.7632.6. The hybrid
build uses React and React DOM 19.2.7, Vite 8.3.8 and TypeScript 5.9.3.

The offline verification runs the ledger, HTTP release, codec and hybrid
tests. It checks the runtime versions, browser, schema digest and cached
hybrid dependencies before testing. Each runner produces fresh evidence in
`target/http-conformance/`.

| Native implementation | Claim | Evidence files |
| --- | --- | --- |
| Rust ledger service | `http.provider` for lookup and transfer | `provider-v1.json`, `provider-v2.json` |
| TypeScript Fetch client | `http.consumer` for lookup and transfer | `typescript-v1.json`, `typescript-v2.json` |
| Standalone Go client | `http.consumer` for lookup and transfer, pinned independently to v1 | `go-v1.json` |
| Rust, TypeScript and Go shape codecs | `json.codec` for round-trip JSON | `shapes-rust.json`, `shapes-typescript.json`, `shapes-go.json` |

The standalone Go client also verifies its own target lock from a temporary
copy with module downloads disabled.

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

The demo covers account lookup and transfer over JSON HTTP using loopback
listeners and fresh in-memory fixtures. Authentication, streaming, cross-origin
browser policy, production rollout orchestration and a database-backed HTTP
service are outside this profile. The shape fixture checks JSON round trips
independently of transport. [HTTP conformance design](../../../docs/superpowers/specs/2026-11-25-http-conformance.md)
