# Typed local command reply plan

Extend the framed local command projection with graph-declared nested reply types and an explicit nullable suffix for ordered sequence input. Preserve the legacy declaration-free format and transport behavior.

- [x] Validate closed record, enum, and list declarations with globally distinct stable identities and acyclic references.
- [x] Lower bounded scalar adapters and map-or-sequence input to deterministic native Serde with ordered object output.
- [x] Replay unrelated Unix IPC cases for positional arity, tag variants, bounded scalar values, and declaration failures. Bind the derived obligation catalog to the projection lock.
- [x] Compose a pinned source-free native status package and compare the extended closure and nested variant cases with the original binary on native macOS ARM64.
