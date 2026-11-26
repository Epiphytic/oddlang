# Bounded local JSON test input

- [x] Project a declared single-value and LF-delimited in-memory JSON contract.
- [x] Recover bounds through native observations and detect a source mutation.
- [x] Verify the unchanged private parser test against the sealed MDK data package.

The unrelated fixture exercises the bounded parser under renamed types and rejects the wrong
provider and unknown error identity. The companion qualification at
`verification/whole/mdk-json-input` independently recovers both limits and guard precedence,
repeats recovery after a source mutation, and runs the same pinned case against original
and generated Rust test targets on ARM64 macOS and Linux. This gate covers the private supplied-input seam.
