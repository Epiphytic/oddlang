# Typed finite error domains

Recover pinned Rust declarations for an equality-capable finite error, its
owned fieldless reason choices, ordered fixed-width named payload fields and
generic Result alias. Keep native names and declaration order bindings
explicit. Project these pure types from portable facts, preserve old finite
error profiles, and verify recovery with independent native callers.

- [x] Verify typed reason, ordered product and diagnostic recovery from
  unrelated source and a mutation, including invalid profile controls.
- [x] Emit the exact native error API and discover previously declared unit
  errors by stable identity in the process-limit package. Replay the
  existing local process-limit profile and exercise shared error linkage.
- [ ] Integrate the pure EngineError family and native BackendChoice
  component into the sealed seven-member workspace. Preserve all
  unrelated member inputs and replay independent upstream consumers.
