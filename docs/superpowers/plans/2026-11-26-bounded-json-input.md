# Bounded local JSON test input

- [x] Project a declared single-value and LF-delimited in-memory JSON contract.
- [x] Recover bounds through native observations and detect a source mutation.
- [x] Verify the unchanged private parser test against the sealed MDK data package.

The unrelated fixture exercises the bounded parser under renamed types and rejects the wrong
provider and unknown error identity. The companion qualification at
`verification/whole/mdk-json-input` independently recovers both limits and guard precedence,
repeats recovery after a source mutation, and runs the same pinned case against original
and generated Rust test targets on ARM64 macOS and Linux. This gate covers the private supplied-input seam.
## Canonical workspace test seam

- [x] Compose the bounded parser with the frozen canonical compiler. Check the test dependency is absent from the ordinary library dependency graph.
- [x] Run the sealed named suite on both native ARM64 hosts and reject an invocation whose exact filter selects zero tests.

The fresh sealed workspace ran 43 of 909 active upstream cases on native
macOS ARM64 and 44 of 887 on Linux ARM64. Both recorded five additional
ignored cases and rejected the successful Cargo invocation whose exact
MDK selector ran zero tests. It adds one credited case to the prior
canonical suite on each host. The parser and provider are test-only.
Both runs used workspace lock
`0bf735fda63690510b21827669e79dcb54a055921f1ee0b91151a9d59f13c49d`.
[macOS receipt](https://github.com/Epiphytic/marmish-oddlang/blob/c8d1fd024e8f59c72cc4a1935b7cc4c1ca04d118/verification/whole/suite/canonical-mdk-aarch64-apple-darwin.json)
· [Linux receipt](https://github.com/Epiphytic/marmish-oddlang/blob/c8d1fd024e8f59c72cc4a1935b7cc4c1ca04d118/verification/whole/suite/canonical-mdk-aarch64-unknown-linux-gnu.json)
