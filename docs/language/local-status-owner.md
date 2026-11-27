# Local status owner

The `oddlang-local-status-owner-v1` profile describes a same-user, read-only Unix endpoint with an application-owned status phase. The graph names the string-tagged, closed request object and the ordered typed response fields. The target emits one u32 big-endian length-prefixed JSON response per connection and half-closes output. Transport bounds cover request size, pathname length, concurrent connections, read time, write time, and accept retries.

```sh
oddlang build local-status-owner rust GRAPH.json RUST.json OUTPUT
```

The Rust binding supplies `format: "oddlang-local-status-owner-rust-v1"`, an executable name, and the long socket option. Native packages use component kind `local-status-owner` and can bind `run_owner` to a binary entry. The companion application chooses the endpoint explicitly. Tokio provides Unix sockets, deadlines and signal handling. Clap handles the entry arguments, libc supplies the effective UID, and Serde validates and encodes closed JSON objects.

The owner binds inside a private directory owned by its effective UID, refuses an occupied endpoint, and restricts its socket mode to 0600. Before accepting a request it verifies the accepted peer's effective UID. Excess peers are closed while admitted connections spend bounded time in the request and reply phases. Invalid requests produce the declared failure reply. The server owns one socket pathname and removes it only while it still refers to the socket it created.

The graph's `lifecycle.bound` phase becomes observable once the owner successfully binds. `BoundOwner::snapshot_publisher` returns a handle that can commit a declared transition and read the current phase. A transition must be reachable from the bound phase. The owning application decides when local effects justify each transition. Shutdown closes admission, sets the dedicated stopping phase, cancels in-flight exchanges and removes the owned socket. Dropping the owner or serving future also closes the lifecycle. The generated executable handles SIGINT and SIGTERM. It does not start services, invent provider readiness, or dispatch any operation other than the selected read-only request.

The [river gauge fixture](../../verification/local-status-owner/) compiles an unrelated executable and native consumer from source-free projections. The consumer publishes and reads successive phases through real sockets. Separate process checks cover framing, malformed or oversized input, admission, deadlines, owner mode, signal exit, and replacement pathname safety.

```sh
cargo build --locked --offline
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin local-status-owner -- target/debug/oddlang
```
