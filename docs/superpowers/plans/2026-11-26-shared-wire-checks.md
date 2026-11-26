# Shared typed wire record checks

Define named, ordered protocol checks over existing record declarations. A call borrows a value of the declared record type and uses the same typed guard expressions and error handling as an endpoint. Check definitions remain separate from their call sites in the obligation catalog.

- [x] Validate owner types, check references, declaration identities and acyclic dependencies, including unused checks.
- [x] Compile reachable functions in deterministic order and preserve warning-free Rust for empty rules and unused declarations.
- [x] Compile and run the unrelated weather package, verify failure order, and compare generated Rust and obligations across reordered declarations. Replay legacy canonical-wire behavior.
