# Local command client

The `oddlang-local-command-client-v1` graph describes one explicit local socket exchange. Graph inputs declare an argument ID for the socket path, ordered native argument guards, ordered JSON request fields, the response schema, and finite process results. The wire is one positive bounded u32 big-endian JSON frame in each direction. The client verifies the connected peer's effective UID before sending payload bytes and half-closes after its request. Connect, peer check, write and reply share one deadline. The generated target uses Tokio Unix sockets, Serde and libc.

```sh
oddlang build local-command-client rust GRAPH.json RUST.json OUTPUT
```

The Rust binding declares `format: "oddlang-local-command-client-rust-v1"` and a simple `entry` identifier. The generated entry takes a Clap `ArgMatches` and returns `ExitCode`. For package assembly, declare the component as `local-command-client`, pin its inputs, reexport its entry, and configure `handlers` on the command surface Rust overlay. A handler maps an existing operation IRI to a root-relative Rust path given as an array of identifiers. The root entry dispatches the matching operation and retains its uncovered status for every unbound operation. Native package compilation verifies the handler signature.

Use `u32_be_json_half_close` framing and `same_effective_uid` peer verification. The response schema requires a boolean success field. Ordered fields can be required, nullable with explicit JSON null, or omitted when null. Unknown or duplicate response fields are rejected. Values can be typed boolean or string, an arbitrary JSON value, or null. The target normalizes valid responses through the schema rather than copying peer bytes into output. Both successful and failed application replies go to stdout with the declared exit status. Transport, binding and frame failures produce one declared stderr diagnostic and exit status. No operation launches a process or locates a service automatically.

The [field meter fixture](../../verification/local-command-client/) builds an unrelated native command and uses a real Unix listener. It compares exact request bytes and half-close, valid and invalid replies, field order, frame bounds, missing socket, a dropped frame, deadline and argument validation. It rejects a graph whose request exceeds its own configured frame limit.

```sh
cargo build --locked --offline
python3 verification/local-command-client/check.py target/debug/oddlang
```
