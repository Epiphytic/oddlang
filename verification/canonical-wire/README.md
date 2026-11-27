# Canonical JSON wire regression

Build the compiler with Rust 1.97.1, then run the unrelated native radio fixture.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
cargo +1.97.1 run --offline --locked --manifest-path verification/provider-validation/Cargo.toml --bin canonical-wire -- target/debug/oddlang
```

The fixture compiles the generated Rust as an edition 2021 module with warnings denied. A handwritten caller checks exact directional bytes, malformed canonical envelopes, separate size and admission errors, validation precedence, timed admission, ordered optional and tagged scopes, external result propagation, byte aggregate limits, hexadecimal fields and independently expected SHA-256 bytes. The build driver checks determinism, verifies emitted hashes, and rejects incomplete mappings, a misplaced clock scope, conflicting declaration IDs and private structural API leaks. A compile-and-run control covers singleton envelopes. A control without operation guards verifies that shape-only byte bounds still reject oversize collections before serialization or hashing.


The independent weather fixture exercises the standalone native package path with a closed canonical schema and generated shared error. It compiles the packaged library and an external caller with Rust 2021 warnings denied. It rejects missing shape pins, shape pins on unrelated components and a digest mismatch in either structural input. It also covers scalar byte limits and a frame whose JSON escaping exceeds the writer bound.

```sh
cargo +1.97.1 run --offline --locked --manifest-path verification/provider-validation/Cargo.toml --bin canonical-wire-package -- target/debug/oddlang
```

A protocol can set `"layout": "flat-sorted"` to put an adjacent union's tag and content directly in its enclosing JSON object. Its UTF-8 wire form uses compact JSON with object keys sorted recursively. Arrays retain their order. The declaration is rejected when the promoted names collide with envelope field names. The radio fixture checks exact bytes, round trips, duplicate keys and noncanonical field order. The default encoding retains the nested selector.

Flat frames order keys by UTF-8 byte order and use serde_json compact string escaping. The packaged Rust fixture pins serde_json 1.0.154. Case digest declarations keep their own declared field order. Exact-limit and one-byte-over fixtures exercise the wire bound after flattening.

Typed protocol steps can use `for-each` with a list `value` path, a lexical `bind` name and nested `steps`. Borrowed items are checked in list order. The first failing condition determines the error. An empty list runs no nested steps. The binding can select a named record member or use an empty field path for the item itself.

The numeric `length` expression counts decoded bytes for `bounded-base64`. Text paths support `text-equal` and `text-prefix` with a literal `text` argument, along with `text-excludes-nul`. The numeric `json-string-length` expression counts the compact JSON string's UTF-8 bytes including enclosing quotes and escaping. It makes one pass without constructing an encoded string and uses saturating unsigned 64-bit arithmetic.

The weather fixture checks nested lists, first-failure order, exact text and prefix checks, Unicode and JSON escape lengths, NUL exclusion and decoded byte counts. It also rejects mistyped operands, a shadowed binding and a duplicate nested declaration.

The typed collection and text guard increment passed both fixture commands above on macOS ARM64 with Rust 1.97.1. The compiler library suite passed all 18 tests with a locked offline build. The process vocabulary and work graph passed Turtle parsing, and the ready-work query ran successfully.

A protocol can declare `recordChecks` as a map from stable check identities to `owner` and ordered `steps`. An owner identifies an internal named record. Inside a check, the `value` scope borrows that record and its field paths use the record's declared field identities. A `check-record` step has its own identity, a `value` path of exactly the owner's record type, and a `check` identity. It propagates the first error from the shared check.

The weather fixture calls shared checks from an endpoint’s nested list traversal. Check dependencies must be acyclic. Empty checks succeed, and unused checks retain their typed obligations while emitting no runtime functions. Adding declarations can change internal generated identifiers. Declaration and call obligations retain distinct identities.

The weather fixture uses a shared check over each nested sample record. It checks chained calls, empty and unused declarations, equal output across reordered definitions, and rejection of unresolved calls, cycles, wrong owners and identity or native naming collisions. Generated Rust compiles with warnings denied.

Both fixture commands passed with the shared-check compiler on macOS ARM64 with Rust 1.97.1. All 18 compiler library tests passed with a locked offline build. The process vocabulary and work graph parsed as Turtle, and the ready-work query completed.
