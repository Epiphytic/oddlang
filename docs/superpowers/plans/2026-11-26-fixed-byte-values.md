# Owned fixed-width byte values

Represent an opaque fixed-width value, construction from an exact array or checked byte sequence, a read-only borrow and erasure of its owned bytes on drop. The graph names the length and failure IRI. The Rust overlay binds the method names. A Rust backend can use a locked zeroize provider. No value formatting, cloning, equality, serialization, or public backing field is part of this profile. Erasure applies to the owned storage at destruction. Caller buffers, past moves or aborting a process are outside that guarantee.

- [x] Extend checked values with a closed fixed-bytes definition. Reject inapplicable bindings and incompatible record nesting. Use the existing stable-ID error binding, package seals and deterministic obligation output.
- [x] Witness an unrelated width, boundary lengths, ownership and negative trait access. Recheck existing checked-value projections.
- [x] Extend the companion vault data package. Qualify the original unchanged strict_public_values test via an external test harness linked to the generated crate. Check the original signatures and drop requirement separately. Keep provider access outside the test. Record upstream structural width recovery separately from authoring the ownership policy.

## Review decisions

Use the existing v2 rustc array observer from Oddlang #59 to recover the selected tuple field's zeroizing owner, octet element and fixed extent. The API names and failure mapping are reviewed bindings; independent compiled callers check them. Re-run recovery after changing an unrelated original extent from 7 to 9. A variable extent or different wrapper is outside the input profile. The profile accepts zero-length arrays and uses an unsigned 32-bit extent. Allocation and target object-size limits still apply.

For the application witness, extract the entire final test module of the pinned vault lib.rs, SHA-256 5389218be38e60288371fe63a79ae71d2e9dbb8408a34bc53e8010f03117359c. Prepend a public crate re-export outside that module so its original super import resolves. Run the one named test and require exactly one pass. Its error variant is the previously qualified provider-free Error::Malformed. For generation and compilation, stage only package inputs and deny access to the original and companion trees.

The Rust package must select the crates.io zeroize 1.9.1 provider and lock its dependency. For erasure, check the selected member shape, native needs_drop, actual invocation of the Zeroizing destructor on an instrumented payload, and zeroization of a known fixed-size byte array through the same provider. Those observations establish the implementation and provider semantics without inspecting the lifetime-ended bytes of a dropped object.

- [ ] Compose the qualified vault package and source-recovered release metadata into the next five-member release. Replay the unchanged strict public values test and independently check exact-array ownership and drop. The separately pinned earlier five-member workspace keeps its existing acceptance evidence.

The [companion qualification](https://github.com/Epiphytic/marmish-oddlang/pull/68) records identical semantic, package and dependency seals for macOS and Linux ARM64. Each host passed the original test in both libraries. A changed array length gave E0277 when compiling the unchanged test module.
