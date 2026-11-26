# Public type bindings in the recovered contract

The source API gate needs to answer whether a downstream caller can name a type and access a field. A definition’s visibility and its usable public spelling are separate observations. Re-exports can give one definition several names, bypass a private parent module, or make a public glob ambiguous. The pinned Rust compiler resolves those bindings for each build profile.

- [ ] Add a typed, source-free public type graph alongside the existing structural projections. Each public binding edge records the local module, public spelling and resolved definition. Start at the crate root and follow public module edges, retaining cycles as edges. Record ambiguous type names, names that resolve outside the selected crate, and unsupported namespaces as explicit limits. The first projection covers module-level type names. Value constructors, macros, trait lookup and external crate internals have separate obligations.
- [ ] Record the declared field audience for local data types, including tuple fields and enum payloads. Module-scoped audience identifies the declaring visibility boundary. Use stable field IDs shared by the shape graph; the public binding graph describes access independently of layout and processing.
- [ ] Exercise public alias through a private module, public and restricted fields, a renamed glob, an ambiguous glob, a module cycle and a feature-selected export in an unrelated crate. Compile positive and negative downstream callers and mutate an alias while keeping the type definition fixed.
- [ ] Run the extractor against the pinned Marmish stream library and check actual external caller spellings. Check the work ontology, package the compiler as an open framework PR, and merge the reviewed companion graph with its receipt.

## Pinned compiler and independent checks

1. Read resolved module children and the separate ambiguous-module-children table. Retain the compiler's primary and alternative type identities for ambiguous globs and keep ambiguity separate from ordinary public bindings. The negative probe sets `deny(ambiguous_glob_imports)` and checks the diagnostic. A second probe permits the lint and checks which identity the compiler selected. The resolver does not retain both visibility choices for `ambiguous_import_visibilities` in its ordinary child table. Record that limitation and test a crate that emits the lint.
2. Treat `pub use ... as ...` as a renamed binding of the same definition. A `pub type` declaration has its own identity. The graph leaves its aliased type expression to the structural/type projections. Keep external crate/definition IDs and explicitly leave external module traversal, hygienic spellings and anonymous imports outside the guarantee. Enum variant re-exports retain their own kind. Struct and tuple field audiences concern reading; enum payload audiences inherit the variant. Constructor privacy, non-exhaustiveness and pattern legality need separate probes.
3. Compile positive name-resolution probes for every unambiguous spelling in scope at a finite public module graph cut. Assign each reachable local module one canonical path from the crate root, choosing first by edge count and then lexicographically by spelling components. Probe every direct public type edge once under that canonical path, including module aliases and back edges. Do not probe descendants through a noncanonical edge. The root path is empty. Independently enumerate the same cut from rustdoc and check its public module entries and give every discrepancy a recorded disposition. Preserve cycle edges and alias-equivalent origins. Negative probes require the intended private-field, private-module, unresolved-path or ambiguity diagnostic. Use isolated feature profiles and reproduce graph bytes in two directories.
4. Compare the original core and stream libraries with their generated counterparts. Core contains public re-exports and stream provides the existing compatibility workload. Also inspect the original engine library's glob re-exports. Report each package and profile separately. The result covers public module-level type names and field reading. Value/variant constructors, macros, associated items, external module internals, full type equality and the rest of the workspace retain separate obligations.

## Graph and probe boundaries

`oddlang recover rust-exports` writes `exports.json` and `exports.lock` for
one locked Rust library profile. Module nodes retain their canonical path.
An unambiguous edge stores its resolution kind and stable target identity.
An ambiguous name has a separate primary resolution and the alternative
resolutions the pinned resolver provides, together with each candidate's
visibility. The pinned table exposes a primary/secondary pair for a conflict,
rather than a complete account of every source-level import in a glob chain.

Rustdoc JSON is a separate observation. Its verifier enumerates type entries
and public imports, saturates local glob exposure, and independently assigns
module paths. It publishes differences with both observations and the outcome
of downstream compilation for document-only names. The field checker tests
reading against concrete data types. It records generic receivers that need
an independently written instantiation as uncovered. Scope and origin identities
remain available in the graph even when an original/generated comparison
only compares the downstream audience and within-crate alias groups.
