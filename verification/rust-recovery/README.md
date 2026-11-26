# Compiler adapter for Rust recovery

Install the compiler's development metadata with rustup before running the adapter.

```sh
rustup component add rustc-dev --toolchain 1.97.1
python3 verification/rust-recovery/test_adapter.py
```

`run.sh` takes an absolute Cargo manifest path, package, `lib` or `bin`,
target name, Rust target triple, `default` or `no-default`, and zero or
more Cargo feature names. For example, for a checkout of Marmish at
`49fc705408a230bd0ad896248d08dbdd2faad05a` on Apple Silicon:

```sh
verification/rust-recovery/run.sh /path/to/marmish/Cargo.toml \
  marmish-stream lib marmish_stream aarch64-apple-darwin default
verification/rust-recovery/run.sh /path/to/marmish/Cargo.toml \
  marmish-prototype bin marmish-engine-bridge aarch64-apple-darwin default
verification/rust-recovery/run.sh /path/to/marmish/Cargo.toml \
  marmish-prototype lib marmish_prototype aarch64-apple-darwin default
```

The adapter uses Rust 1.97.1 at commit
`8bab26f4f68e0e26f0bb7960be334d5b520ea452`. Cargo runs locked, offline and
in release mode with fresh target directories for the adapter and the selected
crate. The shell clears inherited Rust flags and Cargo profile and target flag
overrides. Cargo configuration files, build scripts and proc macros still
participate in the build. Use a trusted checkout. An alternate target needs
its Rust standard library and applicable native toolchain installed. The host
linker needs to support the compiler driver. The selected target's raw JSON goes
to stdout. Cargo diagnostics go to stderr.

The compiler callback visits definitions after cfg selection, name resolution,
macro expansion and type checking. It reads each eligible body's drop-elaborated
MIR before the final optimization passes when that view is still available. A
consulted optimized or const-evaluation body carries a gap. The output contains
declarations, typed function signatures, and declaration-level relationships
such as a resolved call, field access, generic bound or re-export. A trait dispatch
edge names the trait operation. A call edge identifies the implementation when
the compiler resolves a concrete item. A function-reference edge records a
function item's use as a MIR operand, including where a combinator receives it
as an argument. Dynamic, unresolved generic and indirect calls carry distinct
dispatch gaps. The adapter writes no MIR,
statements, source excerpts or token arrays.

Definition paths distinguish Rust namespaces and same-named owners. Numeric
compiler disambiguators and anonymous compiler identities may change when
items are reordered or synthesized differently. An ordinal-identity gap marks
compiler-disambiguated declarations. Nominal self and trait identities alone
cannot distinguish multiple inherent implementations of one type or
implementations of a trait under different generic arguments. Durable identity
for these cases needs canonical type identity or a reviewed mapping. A named
gap marks a compiler name that has no identifier spelling. Source locations and
macro expansion flags provide compiler evidence. Consumers assign identity across
releases separately.

The adapter reports candidate structure. Its gap rows identify incomplete type
shapes and lifetimes, generic predicates, field and variant order, unresolved
dispatch or reads and writes, alias expansions, package instance mapping and
remaining body coverage. Named per-owner gaps record unclassified implicit drops,
skipped inline constants, discriminants and synthetic coroutine bodies, and
unclassified intermediate field projections. Transitive mutations through aliases
and execution reachability remain open. The fixture recompiles a consumer after
removing an unused private item, redirecting a direct call, and substituting a
different function value passed to `map`. It compares the selected caller's direct calls against temporary MIR emitted by
ordinary Cargo without the adapter. `verification/rust-inventory/` provides
a separate nightly Rustdoc declaration inventory. Its compiler, coverage and
Cargo mode differ from this adapter's, so reconcile their overlapping claims
at the declaration level.

## Same-profile declaration comparison

`release_rustdoc.py` runs the compiler adapter and a separate private-item rustdoc JSON pass with Rust 1.97.1. Both must use the pinned compiler commit and requested target. The verifier compares the actual Cargo invocations' edition, crate kind, feature and build-script cfg flags, code generation settings affecting cfg, external dependency aliases and remapped workspace root. It forwards the compiler's optimization and assertion settings because Cargo's release rustdoc command can omit them. The unstable JSON switch is enabled only in the rustdoc subprocess. Rustdoc implicitly selects `cfg(doc)`; the report and fixture preserve that difference. Run the fixture with `python3 -B verification/rust-recovery/test_release_rustdoc.py`.

```sh
python3 -B verification/rust-recovery/release_rustdoc.py /path/to/workspace package \
  lib crate_name aarch64-apple-darwin --out /tmp/declarations.json
```

The semantic comparison uses enclosing declared names and normalized categories. It preserves collisions and counts observations with missing identity by original kind. Declaration witnesses carry each observer's file and starting line. The separate `spanEvidence` section retains the historical diagnostic method. Supply `--previous-spans PATH` with a prior `reconcile_rustdoc.py` report to track each original one-sided key.

A path match verifies the presence of a named declaration at this ownership boundary. It does not qualify its type, layout, body or effects. The current mapping needs additional identity rules for impl-owned items, generic parameters and anonymous or synthetic items. Aliases are compared by name. Dependency instance and content reconciliation remains open. Cargo build scripts can observe their environment, and macro expansion differs between documentation and code-generation interfaces.
## Ordered data shapes

The same pinned Cargo profile can recover a candidate data shape graph.

```sh
cargo build --locked --offline --bin oddlang
target/debug/oddlang recover rust-shapes /path/to/Cargo.toml \
  --profile /path/to/profile.toml --out /tmp/data-shapes
python3 verification/rust-recovery/test_shapes.py target/debug/oddlang
```

Use the `recover rust` profile format. For this adapter include
`Cargo.toml`, `Cargo.lock`, `driver.rs`, `shapes.rs` and `shape_model.rs`
in `adapterInputs` so the lock records all build inputs. The output consists
of `shapes.json` and `shapes.lock`. The lock identifies the selected build and
hashes the graph. Graph identities are scoped to that target. Named members
keep their IDs when moved within the same owner. Tuple fields use positional
identities. Declarations are sorted by ID and field, variant, parameter and
argument positions are zero-based. Source positions are absent from the graph.

The `oddlang-data-shapes-v1` vocabulary describes products, sums and unions
through ordered members, nested types and nominal references. Fixed-width
scalars carry widths in bits. Pointer-sized integers carry the selected
target's width. References identify shared or exclusive borrowing and name
early-bound lifetimes where available. Rust's compiler-recognized `Arc` and
`Rc` contribute shared ownership and `Box` contributes exclusive ownership.
Named types retain the compiler-resolved definition and bind each argument
to its declared parameter. A graph consumer can use these observations to
author a portable type contract and map nominal references to contract IDs.

The graph reports owner-scoped gaps for array lengths, constant arguments,
unsupported types and lifetimes, raw pointer provenance, explicit
discriminants, union active fields and generic constraints or defaults. The
candidate also leaves visibility, export paths, stable workspace package
instances and native layout open. It covers local data declarations for
the selected profile. Library type internals, behavioral effects, generic
satisfaction and compatibility across profiles still require evidence.

### Compare a concrete public value API

Build a public rustdoc JSON document for each library with the same pinned 1.97.1 toolchain, target and flags. Run `oddlang inspect native-value-api FILE.json` on each and compare the output. This independent native witness resolves private-module reexports to their public root names. It records the order of unit enum variants, private tuple arity, inherent method parameter/return types, receiver and borrow mode, associated constant types and non-blanket trait identities including auto traits. Parameter names and private module names can change without changing the comparison. The extractor rejects public shapes outside this small vocabulary. The witness omits trait method bodies and overrides, constant values, layout, attributes and blanket impls. Runtime obligations cover values and behavior.

Run `python3 verification/rust-recovery/test_public_api.py /path/to/oddlang` for the unrelated native fixture. The numeric rustdoc references, source spans and code bodies never enter the witness.
