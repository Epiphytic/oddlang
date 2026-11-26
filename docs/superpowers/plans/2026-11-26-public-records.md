# Public observational records implementation plan

**Goal:** Recover public data records and their re-export identities from selected pinned Rust source and project them as an independent native package component.

**Architecture:** A scoped source selection identifies public module files and record declarations by stable IRI. Recover ordered named fields and their finite types, native clone and serialization declarations, and explicit public re-exports. Keep source bytes and syntax in the recovery input. Publish a portable graph, a Rust identifier overlay, and a separate provenance receipt. A native renderer emits one definition per record and actual public uses for its aliases. The native qualification compares callers linked to the original and generated libraries.

**Constraints:** The source language slice is unconditional named public Rust records, primitive bool and u64, String, Vec<u8>, Clone and Serde Serialize. Generated artifacts are built offline from graphs and overlays. Phase scope excludes journal storage, commands, transport and MDK behavior.

- [x] Build the unrelated fixture first. Recover a record with each admitted field type and a renamed explicit re-export. Challenge conditional declarations, unknown field types, stale pins, duplicate identifiers, and unresolved aliases.
- [x] Project a graph into one deterministic Rust definition and true re-exports. Compile native consumers that check trait, type, serialized field order and two-way assignability. Verify that deleting Rust input leaves graph-only generation viable.
- [ ] Recover the three original native journal records and their existing MDK adapter paths. Check exact source byte pins on recovery and compare the sealed generated library with original callers.
- [ ] Mutate a selected record's field order or type in a detached temporary source copy. Independently verify the changed graph and native behavior. Publish exact reviewed framework and companion evidence and preserve earlier release pins.
