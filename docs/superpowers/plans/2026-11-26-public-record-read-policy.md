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
- [ ] Compare the unchanged downstream caller and all frozen input rows with the
  pinned original and the standalone projected record on both native ARM64 hosts.
- [ ] Change the source to reject unknown keys and compare that observation to the
  projected policy change, preserving other rows and the external choice identity.
