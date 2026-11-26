# Public data record read policy

**Goal**  Declare a public data record's object and ordered sequence reads, optional
values and references to separately projected string choices.

**Architecture**  Extend the existing public record graph and Rust overlay. An
explicit field permits ignoring unknown object keys. When a record supports
deserialization and supplies no such policy, the generated record rejects unknown
object keys. The closed protocol schemas and conservative Rust source recovery retain
their prior behavior.

- [x] Prove default closure, explicit unknown-key handling, optional and positional
  values, and checked external choice linkage with unrelated native cases.
- [x] Compare the unchanged downstream caller and all frozen input rows with the
  pinned original and the standalone projected record on both native ARM64 hosts.
- [x] Change the source to reject unknown keys and compare that observation to the
  projected policy change, preserving other rows and the external choice identity.

Both native ARM64 projections reproduced the unchanged 35-row external
caller and the source mutation's exact diagnostic. The sealed native
package lock is
`884180d21d079c49a7ca0d045b2f56d67700d1666012a92d7b8310f1bb2fe109`.
[macOS ARM64](https://github.com/Epiphytic/marmish-oddlang/blob/77c4c6272bf317e11cf9a2783eb7b00e3fcca0f0/verification/whole/gap-range-record/aarch64-apple-darwin.json)
· [Linux ARM64](https://github.com/Epiphytic/marmish-oddlang/blob/77c4c6272bf317e11cf9a2783eb7b00e3fcca0f0/verification/whole/gap-range-record/aarch64-unknown-linux-gnu.json)

The existing source recovery, value defaults and closed wire-schema fixtures
also pass with this runtime. Native application qualification uses a standalone
candidate and a disposable source mutation of the original.
