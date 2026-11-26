# Structured diagnostics

The `oddlang-structured-diagnostics-v1` profile describes functions that
update one shared boolean, replace a caller-owned text value, call an earlier
text-only function, and emit structured observations. Functions name read-only
and writable text parameters. Their action arrays execute in order. A
`when` action reads the flag at that point and executes its actions when the
flag is true. A `set` always updates the flag, including when it already has
the requested value.

An `emit` writes one compact JSON object followed by LF. Field names are
unique and sorted lexicographically. Text can be literal or read from a
declared parameter. The other field sources are unsigned integers,
booleans, OS, architecture and package version. A failed write does not
change subsequent actions, update the caller's return contract or undo
earlier actions. Records have no timestamps or implicit fields.

The Rust overlay currently selects `{"sink":"stderr","borrow":"static"}`.
The projection uses a relaxed atomic flag, static text references, a locked
standard error stream and the package's pinned `serde_json` dependency.
The component emits a library module through a `structured-diagnostics`
native-package entry, with each public function selectable as a reexport.

```sh
oddlang recover structured-diagnostics MODULE.rs GRAPH.json
oddlang build structured-diagnostics rust GRAPH.json OVERLAY.json PROJECTION
```

Recovery uses a narrow Rust syntax profile. It expects the standalone module's
standard writer and atomic imports, one literal-initialized private atomic
flag, and ordinary public functions. It recognizes simple assignments, the
supported flag operations, ignored writes of a literal-format JSON line and
direct calls. It rejects the unqualified parameter spelling `null`, which the
JSON macro interprets as a literal. Source expressions outside those shapes are rejected. The
reviewed input must resolve the supported provider names to the standard
library and pinned JSON provider in its enclosing crate. Recovery does not
infer arbitrary name resolution, aliases, expressions, I/O or a broader
synchronization contract.

The unrelated fixture builds two versions of an independently written Rust
module and calls both from the same downstream binary shape. It checks exact
stream bytes, closure of the failure path when the sink cannot be written,
repeated activation, visibility after joining an activating thread, and
text updates. Its second source version changes the initial flag and a
message. Both changes reach the recovered graph and emitted binaries.

```sh
cargo +1.97.1 build --release --locked --offline
python3 verification/structured-diagnostics/check.py target/release/oddlang
```
