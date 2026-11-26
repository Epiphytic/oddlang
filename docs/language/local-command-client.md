# Local command client

The `oddlang-local-command-client-v1` graph describes one explicit local socket exchange. Graph inputs declare an argument ID for the socket path, ordered native argument guards, ordered JSON request fields, the response schema, and finite process results. The wire is one positive bounded u32 big-endian JSON frame in each direction. The client verifies the connected peer's effective UID before sending payload bytes and half-closes after its request. Connect, peer check, write and reply share one deadline. The generated target uses Tokio Unix sockets, Serde and libc.

```sh
oddlang build local-command-client rust GRAPH.json RUST.json OUTPUT
```

The Rust binding declares `format: "oddlang-local-command-client-rust-v1"` and a lowercase `entry` identifier. The generated entry takes a Clap `ArgMatches` and returns `ExitCode`. For package assembly, declare the component as `local-command-client`, pin its inputs, reexport its entry, and configure `handlers` on the command surface Rust overlay. A handler maps an existing operation IRI to a root-relative Rust path given as an array of identifiers. The root entry dispatches the matching operation and retains its uncovered status for every unbound operation. The link step resolves each handler to an exported local command client and verifies every socket and guard parameter against the CLI root argument IDs and types. A command tree with descendants or external commands must declare those arguments global so child options cannot shadow them. Native package compilation checks the generated signature.

Use `u32_be_json_half_close` framing and `same_effective_uid` peer verification. The response schema requires a boolean success field. Ordered fields can be required or optional. Optional fields accept missing or null input. Output either writes explicit JSON null or omits the field as declared. Unknown or duplicate response fields are rejected. Values can be typed boolean or string, an arbitrary JSON value, or null. The target reserializes valid responses through the schema. It consumes the first complete reply frame and leaves trailing data unused. Both successful and failed application replies go to stdout with the declared exit status. Transport, binding and frame failures produce one declared stderr diagnostic and exit status. No operation launches a process or locates a service automatically.

## Typed replies

Typed responses have a root IRI in `response.id` and an IRI for every field, nested declaration, and variant. Field order determines both the sequence input order and the JSON object output order. `ordered_sequence_minimum` declares how many leading positions a positional reply must provide. Its remaining fields must be nullable. Map inputs still accept omitted nullable fields. `default: true` mirrors a nullable nested member that also defaults when its positional record omits it.

`response.declarations` supplies the types reached by explicit `{"kind":"ref","id":"…"}` references. Supported declarations include closed ordered records, closed string enumerations, and tagged enumerations with unit or ordered named cases. A tag alone denotes an internally tagged enumeration. An additional `content` key denotes an adjacent tagged representation. Internally tagged unit cases discard extra payload fields, while named cases reject unknown fields. Nullable fields serialize as null or are omitted according to `presence`.

Primitive types include unsigned integers of 8, 32 or 64 bits. Shape forms include typed lists, fixed byte arrays, fixed-width lowercase hex text, bounded ASCII atoms with declared punctuation, literal unsigned bytes and bounded canonical padded Base64 strings. Base64 adapters use the target's pinned Base64 crate. The transport frame limit applies before decoding all of these shapes. Declaration identities must be unique and references connected and acyclic. The generator publishes `reply-obligations.json` and records its digest in `projection.lock`. The catalog lists requirements for independent qualification.

The [field meter fixture](../../verification/local-command-client/) builds an unrelated native command and uses a real Unix listener. It compares exact request bytes and half-close, valid and invalid replies, field order, frame bounds, missing socket, a dropped frame, deadline and argument validation. It rejects an oversize request graph and links with unknown or mistyped argument IDs or no matching handler.

```sh
cargo build --locked --offline
python3 verification/local-command-client/check.py target/debug/oddlang
```
