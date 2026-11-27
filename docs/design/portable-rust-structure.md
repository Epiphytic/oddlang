# Rust structural recovery evidence

The release-mode Rust 1.97.1 compiler adapter processed the pinned project
from the boundary matrix. It ran from fresh Cargo target directories with
the lockfile and default features. The published graphs contain named
declarations, typed function signatures, dependency edges and explicit
gaps. Compiler source locations remain in extraction evidence. [Boundary](portable-rust-boundary.md)
· [Inventory vocabulary](../language/rust-structure.md)

| Selected target | Declarations | Relationships | Gap rows |
| --- | ---: | ---: | ---: |
| Stream library | 3,296 | 9,534 | 2,976 |
| Bridge binary | 6 | 32 | 8 |
| Prototype library with bridge processing handler | 1,196 | 3,591 | 987 |

The selected bridge binary calls the prototype library's `serve` function
through an external compiler identity. The separate library inventory
contains the corresponding declaration, the request handler and dispatch.
The inventories remain candidates. Function-valued operands record a separate reference edge. Implicit drops, skipped bodies, intermediate projections and compiler ordinal identities carry per-owner gaps. The reported gaps cover incomplete
type shapes and lifetimes, declaration ordering, generic predicates,
compiler-generated identities, package instance binding, dynamic or
generic dispatch, and inaccessible or optimized bodies. Static
relationships do not establish runtime reachability.

Two fresh CLI recoveries of the stream library produced identical
`structure.ttl` and `structure.lock`. The graph SHA-256 was
`b4246a9713db14cb7a95ac528c9150ae786fc1dfa8685fc564b479fa35575c8c`.
The installed toolchain and profile are recorded in
[the adapter instructions](../../verification/rust-recovery/README.md).
Use the same adapter and Cargo lock when comparing a fresh result.
The adapter executable is produced from Oddlang's revision. The lock records the checked source revision, manifests, entry script, adapter source inputs and resulting graph. [Profile](../../examples/marmish/portable-contract/structure.toml) · [Structure lock](../../examples/marmish/portable-contract/structure.lock.json)

The independent Rustdoc comparator matched 378 ordinary declaration keys,
with six compiler-only and 145 Rustdoc-only keys. Including compiler-marked
macro expansions matched 451 keys, with 373 compiler-only and 72
Rustdoc-only keys. These are deduplicated path, starting line, name and normalized kind keys. Counts use the independent collector's library coverage. Differences include compiler versions, release vs
development mode, macro span collisions and differing observation scopes.
The comparison leaves these rows unresolved.

The unrelated fixture exercises macro-expanded declarations, aliased
dependencies, feature-gated items, re-exports, same-named owners and
dynamic dispatch. Removing an unused private declaration and redirecting a
private call both change the inventory while unchanged consumers still
compile. Ordinary Cargo MIR, produced without the extraction adapter,
independently confirms the original and redirected direct calls. The
fixture also checks path identity after a formatting change.

```sh
cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin rust-exports -- qualify adapter
verification/rust-recovery/run.sh /path/to/marmish/Cargo.toml \
  marmish-stream lib marmish_stream aarch64-apple-darwin default > /tmp/compiler.json
cargo +stable run --locked --offline --manifest-path verification/rust-inventory/Cargo.toml -- /path/to/marmish \
  marmish-stream /tmp/rustdoc-inventory
cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin rust-exports -- reconcile /tmp/compiler.json \
  /tmp/rustdoc-inventory/rust-inventory.json
target/debug/oddlang recover rust /path/to/marmish/Cargo.toml \
  --profile examples/marmish/portable-contract/structure.toml --out /tmp/stream-structure
cargo test --locked --offline --test rust_recovery
```

## Core library declaration ownership

At source revision `af54339d250290490d62d4475a77539b7e739071`, release rustc and Rustdoc independently identified 29 unambiguous declaration keys in `marmish-core`. Each key combines the enclosing declaration names with the item kind. The compiler also reported two anonymous constants under one ambiguous key that Rustdoc omitted. In the standalone core package generated from the default seven-member workspace, the corresponding counts were 89 and one. macOS ARM64 and Linux ARM64 produced identical declaration comparisons for each input.

The two inputs share three unambiguous lexical keys: the crate root and the `config` and `frame` modules. The original defines `Limits` directly in `frame`. The generated package defines it in `frame::__projection_2` and exposes it through a re-export. On macOS ARM64, an independent downstream crate compiled the public imports for all six types against the default seven-member workspace. The original declares `ConfigError`, `Retention` and `RelaySet` in `config`, and `FrameError`, `Limits` and `ProbeEnvelope` in `frame`. All six generated declarations have an additional private module in their lexical path. [Generated re-export declarations](https://github.com/Epiphytic/marmish-oddlang/tree/10926d822bb9aa3a16b20ef35cfc63bebd586780/verification/whole/core-lexical/export-facade). Inside each input, the observers agreed on declared access for all 17 eligible original declarations and all 53 eligible generated declarations.

Track original ownership, the portable component, and the target's declaration path independently. A successful downstream import does not explain where the original type was declared. Conversely a new private module in generated Rust does not by itself identify a behavioral difference. The source-free member is bound by package lock `b30e3ad85232743da965f0ca1d823baea25095d66e883005ceb1f790ee44794c` in workspace lock `5abf57c811d641ba126c32dde208334d85d47374bbfbb983231783a403f24df4`. The member lock pins the generated Cargo.lock digest separately. [Archived member lock](https://github.com/Epiphytic/marmish-oddlang/blob/10926d822bb9aa3a16b20ef35cfc63bebd586780/verification/whole/core-lexical/package.lock).

The companion ownership graph assigns six stable type IRIs to two portable modules. Each type can point directly to its semantic domain, its aggregate error cases, or the parameter values on a shared domain. For example, the retention type selects the optional unsigned range. The projection check resolves that type IRI, passes only the selected domain graph to the TypeScript emitter, compares its projection lock and runs the existing Node boundary checks. This qualifies one executable consumer of the lookup. [Portable ownership](https://github.com/Epiphytic/marmish-oddlang/pull/109) and [semantic query and projection](https://github.com/Epiphytic/marmish-oddlang/pull/110).

Across the same six core types, both ARM64 hosts observed the same ten public inherent member names in the original and generated libraries. Independent callers compiled six function-pointer assignments and four typed constant references in each library. Both rejected a missing member and an incompatible constant type. The portable member map links each operation or limit to its owner. The observation covers concrete local inherent declarations indexed by Rustdoc with hidden items enabled under `cfg(doc)`. [Pinned member map and callers](https://github.com/Epiphytic/marmish-oddlang/tree/9caf478ba084416195fbf7f3279eb516d44478a3/verification/whole/core-inherent).
