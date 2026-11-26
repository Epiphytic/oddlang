# Typed arguments in local command requests

> **For agentic workers:** Use superpowers:executing-plans for the implementation and its verification.

**Goal** Extend the local command client projection so an existing CLI handler can send a bounded framed request containing typed argument values.

**Architecture** The existing request remains an ordered list of fields. Each field declares either a literal JSON value or a root CLI argument and its type. A native package links argument IDs and types against the CLI surface before writing Rust. The projected handler serializes the ordered fields, refuses an oversized request before opening the socket, and uses the existing same-user transport and typed reply checks.

**Spec** The basic-command [interop plan](2026-11-26-basic-command-interop.md) records the live peer gates. This primitive supplies part of the client-side local owner handoff. Application-specific configuration, prepare/submit sequencing, owner execution and peer access remain subject to their own contracts.

- [x] Verify the generator rejects an unrelated field-meter fixture while typed request input is unsupported.
- [x] Project string, repeated strings, unsigned 64-bit and flag argument values into the bounded ordered JSON request. Verify the same exact frames and protocol behavior against an independent Unix peer. Exercise missing argument values, frame overflow before opening a socket, mistaken option types, and a literal null.
- [x] Verify static-input compatibility, update the work ontology, and review the change in Claude auto mode.

The graph entry `{"id":"urn:field:request:sensor","key":"sensor","argument":{"id":"sensor","kind":"unsigned64"}}` writes the exact JSON integer supplied by root argument `sensor`. A literal remains `{"key":"sensor","value":10}`. Explicit JSON null is a literal. A field must have one source. Only explicit CLI values satisfy an argument source. CLI defaults do not change the request. Identified request literals use integer JSON numbers, including in nested arrays and objects. An argument-bound request supplies a stable `request_id` IRI and an `id` IRI for each field.

`string` and `unsigned64` require values at invocation. An absent `flag` emits false and an absent `repeated_string` emits an empty array. Repeated strings retain CLI order. A dynamically bound projection publishes each ordered field obligation and the frame limit in `request-obligations.json` and pins the catalog digest in `projection.lock`. Native package linking checks each binding. Standalone callers can read the catalog to supply matching arguments.

[Native verification](../../../verification/local-command-client/typed-request-verification.md)
