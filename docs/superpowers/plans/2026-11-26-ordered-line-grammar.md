# Ordered line record grammar implementation plan

**Goal** Generate exact native parsers for bounded text records from explicit
portable grammar and output declarations.

**Architecture** The grammar declares byte framing and ordered line tokens.
Each token consumes its full span and contributes named typed captures as
specified. Finite tuple and checked aggregate predicates validate the
captures. A Rust overlay binds target symbol names, native record
construction and the stable parser failure. Existing literal-prefix
decimal text subgrammars remain reusable.

- [x] Exercise the new route with an unrelated inventory record fixture.
      Define the graph, native capture overlay, generated Rust caller,
      and malformed/boundary inputs before adding the projector.
- [x] Generate byte framing and typed ordered row captures from
      `src/line_grammar/model.rs` and `src/line_grammar/rust.rs`.
      Check malformed schemas, pin deterministic output, and replay the fixture.
- [ ] Add finite constraints and typed native output records, including
      fixed arrays, bounded by-source vectors, stable native errors, and
      text formatters. Exercise each with independent native callers.
- [ ] Corroborate the application record shapes and limit mutation with
      pinned native compiler evidence. Verify both upstream and emitted Rust
      against the independently published 224 original manifest vectors.
      Review and publish the framework and companion branches separately.

The application role takes bytes and returns release metadata or one
stable error. It includes pure identity and download-name formatting.
