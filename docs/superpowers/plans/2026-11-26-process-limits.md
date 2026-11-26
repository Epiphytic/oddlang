# Native process limits implementation plan

**Goal:** Project an independently pinned engine process-limit operation and run it from the real client and daemon entrypoints.

**Architecture:** A portable resource policy declares the resource identity, platform behavior, ceiling, stable failure identity and preservation contract. Rust binds that failure to a named public unit error and invokes libc only for target policies that lower a soft limit. A binary startup binding names an existing workspace sibling operation and the error exit and stderr bytes. Rust type-checks its unit-result signature and the existing CLI's exit-code signature.

**Constraints:** Use the frozen v7 artifact as the composition baseline. Build and test locked/offline without upstream sources visible to generation. The engine package scope is its process-limit operation and reachable error. Worker startup and spawn checks are separate obligations.

- [x] Add a component fixture that exercises a non-Marmish ceiling, Linux no-op, unchanged descriptor flags, repeated calls and invalid claims.
- [x] Lower the checked resource graph in the native package assembler. Check supported OS policy and representable limits, preserve the hard limit, and read back the result.
- [x] Add a typed workspace sibling startup binding. Exercise the error exit and success-to-entry order and reject a missing edge or invalid signature.
- [x] Compose a separate engine workspace release. Compare high, equal and low resource observations with upstream in children, and check the real binary startup paths on both native ARM64 hosts. Publish independently reviewed framework and companion evidence. [Native ARM64 receipts](https://github.com/Epiphytic/marmish-oddlang/pull/65)
