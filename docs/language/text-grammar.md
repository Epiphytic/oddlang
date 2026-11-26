# Literal prefix and decimal text grammars

An `oddlang-text-grammar-v1` graph declares a stable IRI, a closed ordered list of literal prefixes, the required `body` and an optional `suffix`. A suffix has a nonempty literal delimiter and its own decimal body. The graph declares `consumption: "entire-input"`.

Use the first matching prefix in declared order. After removing it, split at the first suffix delimiter when the graph provides one and the text contains it. Validate each resulting decimal part in full. The `canonical-ascii-decimal` notation admits a nonempty sequence of ASCII digits. Leading zeroes are allowed only for the single digit `0`. Each part specifies an inclusive minimum and an unsigned width of 8, 16, 32 or 64 bits. Overflow and other remaining text fail admission.

`oddlang build text-grammar rust GRAPH OUTPUT` emits the pure Rust function `accepts(&str) -> bool`, a deterministic projection lock and the portable qualification obligation.

`oddlang recover text-grammar rust SOURCE SELECTION OUTPUT` takes a digest-pinned Rust source file and a JSON selection with `id`, `entry` and `sourceSha256`. The entry has a closed `strip_prefix` chain, a `split_once` suffix and direct calls to one canonical decimal helper. Recovery checks the helper's digit guards, unsigned parse and optional nonzero requirement. It infers the prefix priority, delimiter, width and both minimum values. The selection supplies the graph identity and entry function name.

Recovery rejects a different control-flow shape, inconsistent guards, ambiguous or shadowed bindings visible in the selected file, and module-level macro expansions. The recovery lock records native qualification of primitives and prelude resolution outside the selected source file as an obligation.
