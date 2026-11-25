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

## Running a participant

`oddlang wasm run BUNDLE LOCK CLAIM COMPONENT OUT` validates the component, extracts its final WIT, checks the resolved import and export names, and checks the component against the release's selected world. Set `WASM_TOOLS` and `ODDLANG_WASM_HOST` to the prepared executables. The offline verification script builds both. Every case gets a fresh Wasmtime store, a new component instance and only the explicitly declared import bindings. The host receives the scripted import responses and call inputs. The verifier reads the expected outputs independently from the pinned release.

A claim is a JSON object containing a nonempty `participant` identifier, a `roles` array, the selected WIT `world` name, and lowercase SHA-256 hex digests in `releaseSha256`, `componentSha256` and `runnerSha256`. These identify the release manifest, final component bytes and host executable. Both roles refer to the guest. For `wasm.exporter` the host calls its exports. For `wasm.importer` the host observes the imports the guest invokes. A case that covers either selected role runs in full, preserving the order of calls and all host interactions.

The evidence file records the claim, schema and rule set versions, and one observation per selected case. `imports` records the complete ordered transcript as function, arguments and returned value. `calls` records each exported function, its arguments and its lifted result or trap. A trap ends that instance's case. JSON uses decimal strings for s64, `{"none": true}` or `{"some": value}` for options, and `{"ok": value}` or `{"err": value}` for results. Unit payloads use JSON null. A failing participant still produces observations for inspection. Run `oddlang wasm verify-evidence BUNDLE LOCK CLAIM EVIDENCE` to compare the entire selected corpus with the report. An independently held claim and lock establish which artifacts the report must identify.

The bundled typed host supports ledger `lookup` and `transfer`, the `initial-state` import, the test world's `fixture-load` and `complete-snapshot`, and the echo matrix's `echo`, `mirror` and `seed` signatures. It rejects unsupported bindings. To exercise another published world, bind its WIT with your native component tools, supply its declared imports, and invoke the portable `cases.json` in fresh instances. Record the same typed observations and pin your host in the claim. The evidence verifier consumes this format independently of the host implementation.

`oddlang wasm matrix HOST HOST_LOCK GUEST GUEST_LOCK CLAIM COMPONENT DIRECTION OUT` runs the host release's selected cases against a component checked against the guest release. Supply the releases in host/guest order for either `old-host-new-guest` or `new-host-old-guest`. The report records both manifest pins, the direction, the component and runner pins, and selected case IDs. A link, type or behavior failure identifies the failing stage. An `observed-compatible` verdict applies to the reported cases for that exact pairing.
