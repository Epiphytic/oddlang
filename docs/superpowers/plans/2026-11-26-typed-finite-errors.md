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
- [x] Integrate the pure EngineError family and native BackendChoice
  component into the sealed seven-member workspace. Preserve all
  unrelated member inputs and replay independent upstream consumers.

The sealed build passed on native macOS and Linux ARM64 with runtime
`bbdc69e026ee0ae0c50bd9884fa4b6eaad8040a1`. Both native consumers
matched all 26 error categories, three reason enums and 38 compile
negatives. The same verified workspace matched the independent native
BackendChoice caller. Replay preserves the earlier dependency lock,
all sibling edges and five unrelated member rows. The macOS source
mutation changes the diagnostic text, reason ordinals and named product
Debug order in both compiled original and projection.
