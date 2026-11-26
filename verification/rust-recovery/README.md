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
