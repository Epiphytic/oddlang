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
python3 verification/rust-recovery/test_adapter.py
verification/rust-recovery/run.sh /path/to/marmish/Cargo.toml \
  marmish-stream lib marmish_stream aarch64-apple-darwin default > /tmp/compiler.json
python3 verification/rust-inventory/inventory.py /path/to/marmish \
  marmish-stream /tmp/rustdoc-inventory
python3 verification/rust-recovery/reconcile_rustdoc.py /tmp/compiler.json \
  /tmp/rustdoc-inventory/rust-inventory.json
target/debug/oddlang recover rust /path/to/marmish/Cargo.toml \
  --profile examples/marmish/portable-contract/structure.toml --out /tmp/stream-structure
cargo test --locked --offline --test rust_recovery
```
