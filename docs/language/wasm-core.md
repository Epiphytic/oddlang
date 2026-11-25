# Synchronous WebAssembly contract profile

A release selects one WIT world from one explicitly versioned package. The ontology gives each declaration an absolute IRI. WIT names identify ABI members inside their respective scopes. A rename can keep the same IRI. The ontology vocabulary is `urn:oddlang:wasm:`.

| Class | Required facts and permitted relationships |
| --- | --- |
| Package | Exactly one per source. Its name has the form `namespace:package@version`, with a WIT semantic version. |
| Interface | Declares records, enums and variants with `type`, functions with `function`, and referenced types with `use`. |
| World | Imports and exports functions or named local interfaces with `import` and `export`. Refers to named types with `use`. A local `include` inherits another world's declarations. Exactly one world carries `entry true` in a multiworld source. |
| Function | Zero or more `param` edges. Optional `result` names its single WIT result type. Omission means no result. |
| Param, Field | Each has an integer `ordinal` and a `valueType` string. |
| Record | Its `field` edges identify named fields. |
| Enum, Variant | Their `case` edges identify named cases. |
| Case | Has an integer `ordinal`. A variant case may carry one payload described by `valueType`. |
| ForeignType | Its name selects a type from the version-qualified interface path in `from`, for example `example:types/shapes@1.0.0`. The release lock and cache must supply the owning WIT package by exact manifest digest. |

Each subject has exactly one class and one string `name`. The optional string `label` is private descriptive metadata. World, interface and member names follow the WIT identifier grammar. Specify bare names in the ontology. The renderer escapes WIT keywords with `%` in declaration names and type references. Named type references sharing a spelling with a built-in type require an explicit escape, such as `%string`. Every parameter, field and case belongs to one declaration. Within that declaration its ordinal is unique and the ordinals span zero through the number of members minus one. Ordinals determine call order, field layout and discriminant tags. Other declaration sets are emitted in IRI order, independent of Turtle triple order.

Type expressions support `s8`, `s16`, `s32`, `s64`, `bool`, `string`, scoped named types and nesting of `list<T>`, `option<T>` and `result`. A result can carry neither payload, only success (`result<T>`), only error (`result<_, E>`) or both (`result<T, E>`). Named records, variants and enums are defined in interfaces. The synchronous profile rejects unsupported constructs, including resources, handles and async functions. Supply a text WIT directory to the importer. A core Wasm module has no recoverable WIT world through this entry point.

The optional integer predicates `min` and `max` on signed parameters, fields and case payloads are inclusive semantic constraints. Each bound must fit the declared signed width and the lower bound must be no greater than the upper bound. WIT carries only the signed width. The pinned ontology and behavioral cases carry stronger value requirements.

## Diagnostics

A recognized WIT construct outside the profile produces a diagnostic beginning with `unsupported Wasm construct`, followed by the construct name. A binary passed to the WIT directory entry point produces `unsupported Wasm input: expected a WIT directory`. Parser errors for malformed text begin with `invalid WIT`. Ordered declarations distinguish `invalid or missing ordinal`, `duplicate … ordinal` and `noncontiguous … ordinals`. Identity recovery reports missing, conflicting and unused stable IRI mappings separately. A failed recovery keeps existing output intact.

## Publication and recovery

Publication takes a source directory containing `model.ttl`, `dependencies.json` and `corpus.json`. Its WIT resolves the world includes to their effective declarations so inherited types are in scope for new functions. The original include edges remain in the bundled Turtle. A dependency entry with `identity` and `includeWorld` selects a world from an exactly pinned release. Its inherited interfaces retain a qualified `from` path in the composed ontology and their member IRIs. For example, the test ledger imports the production `oddlang:ledger/ledger-types@1.0.0` interface. Its compiled component therefore requests that exact interface from a host. Pin the published manifest to retain bounds, labels, behavior and authorship of declarations across releases. Structural witnesses associate surface paths with IRIs. Behavioral witnesses identify a function and its role relative to the component, either `wasm.importer` or `wasm.exporter`.

The identity JSON object maps paths to absolute IRIs. The paths `package` and `world` identify the package and selected world. For example, `world/export/transfer/param/amount` identifies a parameter. A declaration also reachable through a `use` edge or interface binding has the same IRI at each alias path. An explicit map must cover every path exactly and agree at aliases. As an alternative, the caller can pass a namespace beginning with `urn:` or `https://`. The importer appends the canonical path verbatim, so include a trailing separator in the chosen namespace.

`oddlang wasm import-wit WIT_DIR WORLD ID_MAP_OR_NAMESPACE OUT [--lock WORKSPACE_LOCK]` produces a structural `model.ttl`, an `identities.json` map and a `missing-evidence.json` report for behavior and constraints. Self-contained WIT can be imported directly. For a released world with foreign types, pass the published `wit/` directory and an independently held workspace lock outside the release. The importer stages the manifest-verified WIT bytes, verifies transitive package pins, checks the selected world and confirms that an explicit identity map matches the release. A caller namespace allocates fresh identities instead. Recovered WIT supplies structural facts only. To publish recovered facts as a new contract, supply authored cases and a dependency declaration.

## Directional comparison

The diff compares the selected worlds addressed by the two verified pins. It reports declaration IRIs, old and new WIT surfaces, and the host or guest endpoint responsible for satisfying a changed requirement in the selected direction. A new required import needs a matching host definition when a new guest is linked to the old host. A new export available only on the new guest places no obligation on the old host. ABI-relevant ordinals participate in the comparison. Private labels and semantic bounds do not change the WIT surface.

The structural verdicts are `surface-equal` and `surface-changed`. Neither verdict establishes that a particular binary links or produces the specified behavior. That claim requires execution evidence tied to the same release pins, participant, direction and cases.
