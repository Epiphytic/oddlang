# Nullable unsigned value domains

An `OptionalUnsignedRange` describes a value which may be absent. Supplying a present value checks an inclusive interval. A value outside the interval produces the IRI named by `rangeError`. Absence is a valid explicit input. Requesting the default yields the separate present `defaultValue`. The type's maximum is observable through the native interface.

A project directory supplies `domain.ttl`, using the `urn:oddlang:core:` vocabulary. Declare exactly one domain with a named IRI and the properties `bits` (8, 16, 32 or 64), `minimum`, `maximum`, `defaultValue` and `rangeError`. Integer properties use Turtle integers. Bounds must fit the unsigned width and contain the default. Declare the referenced error as a named `NamedError` with one string `message`. Other named errors in the graph belong to the same error type and retain their separate messages. Unknown declarations, predicates and unresolved references fail validation.

The Rust binding supplies the domain IRI, public type and method identifiers, associated maximum and the mapping between error IRIs and variants. A target may use a different binding or follow the neutral rules directly. The native TypeScript projection uses bigint for exact 64-bit values, null for absence and error IRIs for failures.

```sh
oddlang build value-domain rust PROJECT rust.json NEW_OUTPUT
oddlang build value-domain typescript PROJECT NEW_OUTPUT
```

Each projection emits native source, a digest lock and the same language-independent test obligations. Obligations encode integers in decimal strings. The lock binds the canonical domain, target-specific source and optional binding. This vocabulary covers construction, inspection, default, maximum and range failure. Other operations require their own semantic rules.


## Recover from an original Rust API

`oddlang recover value-domain MANIFEST SELECTION.json NEW_OUTPUT` runs a typed observer against the chosen library under Rust 1.97.1. The selection binds the clean original Git revision, Cargo.lock digest and compiler commit, identifies the public API through the Rust binding, specifies the unsigned width and identifies the range error. The original is compiled in release with its default features, locked and offline. Use this on a trusted native checkout. Build configuration and native dependencies participate in the original build.

The observer reads the default and associated maximum, locates the first accepted value under the interval hypothesis and checks round trips, absence, rejection identities and error messages. It checks every input at 8 and 16 bits. For larger widths it checks edges and reproducible samples. The resulting graph is a candidate for review; a disconnected unobserved accepted interval is possible in a sampled domain. The source pin and measured scope are in `observation.json`. An unrelated fixture changes the source maximum and then introduces an interior rejection. The first change updates the recovered graph; the second fails exhaustive recovery.

To verify a published projection against its graph, run `oddlang verify value-domain rust PROJECT rust.json RELEASE` or `oddlang verify value-domain typescript PROJECT RELEASE`. The verifier reproduces the lock, source and obligation bytes in temporary storage.

A Rust binding may declare `variantOrder`, a list containing each error IRI exactly once. The projection uses it as the native fieldless enum declaration order. Without a declared order the projection uses IRI order. Rust callers can observe the distinction by casting a variant to an integer.
