# Nullable unsigned value domains

An `OptionalUnsignedRange` describes a value which may be absent. Supplying a present value checks an inclusive interval. A value outside the interval produces the IRI named by `rangeError`. Absence is a valid explicit input. Requesting the default yields the separate present `defaultValue`. The type's maximum is observable through the native interface.

A project directory supplies `domain.ttl`, using the `urn:oddlang:core:` vocabulary. Declare exactly one domain with a named IRI and the properties `bits` (8, 16, 32 or 64), `minimum`, `maximum`, `defaultValue` and `rangeError`. Integer properties use Turtle integers. Bounds must fit the unsigned width and contain the default. Declare the referenced error as a named `NamedError` with one string `message`. Other named errors in the graph belong to the same error type and retain their separate messages. Unknown declarations, predicates and unresolved references fail validation.

The Rust binding supplies the domain IRI, public type and method identifiers, associated maximum and the mapping between error IRIs and variants. A target may use a different binding or follow the neutral rules directly. The native TypeScript projection uses bigint for exact 64-bit values, null for absence and error IRIs for failures.

```sh
oddlang build value-domain rust PROJECT rust.json NEW_OUTPUT
oddlang build value-domain typescript PROJECT NEW_OUTPUT
```

Each projection emits native source, a digest lock and the same language-independent test obligations. Obligations encode integers in decimal strings. The lock binds the canonical domain, target-specific source and optional binding. This vocabulary covers construction, inspection, default, maximum and range failure. Other operations require their own semantic rules.
