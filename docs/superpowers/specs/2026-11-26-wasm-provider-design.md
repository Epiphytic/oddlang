# Replaceable Oddlang providers

## Intent

A workspace can select a source-language reader, a language target, or a library binding independently of the Oddlang host build. The provider interface specifies the data a plugin receives and the effects it may request. Oddlang ships fast native Rust implementations. Users can supply independently built Wasm components. Cargo and any compiler or build script it invokes run under the caller's existing build sandbox.

## Interface and authority

Publish versioned WIT worlds for recovery and generation. Recovery receives a bounded list of host-selected source files with logical relative names and bytes, a selection, and a contract identifier. It returns a candidate graph, source evidence and explicit gaps. Generation receives validated contract and overlay documents with their identifiers and returns artifacts with relative names and bytes, declarations of exports and dependency requirements, or structured diagnostics. Version and validate the document schemas carried as bytes. A library binding can be declarative data or a component implementing the applicable world.

The initial worlds import no host capabilities. The Rust host validates actual component imports and exports before instantiation and supplies no WASI, environment, filesystem, network or process handles. Bound memory, fuel, and input and output sizes, including aggregate file count and lengths. The host rejects invalid paths, collisions, unsupported dependencies and invalid results before atomically publishing a fresh output tree. A component can still return semantically incorrect or malicious source. A successful invocation authorizes neither compilation nor execution of its artifacts. Existing trusted build commands can continue invoking Cargo in the caller's environment.

## Selection and compatibility

Resolve a provider by role, capability ID and supported contract version. Use an explicit workspace override or the shipped default. Reject ambiguous or incompatible selections. A lock binds external component bytes, interface version, configuration and relevant inputs. Native defaults are identified by compiler release and capability. Read historical workspace manifests using their original default behavior and preserve their existing workspace locks. Record new provider selections in a separately versioned receipt. Run native implementations via the same logical request and response validation. Plugins can use any guest implementation language that produces the required component.

The design chooses Wasm components over a general executable plugin protocol because an executable inherits OS authority at startup. A native-only Rust trait would require relinking Oddlang for third-party extensions. The internal Rust interface can use Rust types and adapters to the WIT representation.

A workspace registers additional capabilities through explicit provider manifests. Each declares the capability ID, role, supported versions, pinned data schemas and implemented WIT world. The host validates schemas with its supported declarative schema vocabulary and applies resource bounds during validation. A new capability can register without changing the host if it fits an existing world and schema vocabulary. A new host capability requires an explicit interface revision.

## Delivery and acceptance

- [ ] Extract public-record generation behind the host interface. Replace it with a Wasm provider by changing workspace configuration and run the same independent consumer cases against both.
- [ ] Register a capability unknown when the host was compiled and process its fixture without rebuilding Oddlang.
- [ ] Provide an equivalent replacement test for source recovery and an example of selecting a library binding. Each example ships its contract version, WIT world, provider lock and expected results.
- [ ] Test rejection of undeclared imports, resource exhaustion, malformed results, duplicate output paths and incompatible versions. A failure leaves no published partial output.
- [ ] Inventory project-authored extraction, generation and verification tools and their callers. Port them to Rust or Wasm starting with the four active Marmish command-policy scripts. Keep externally established test expectations independent of the generators. Inherited Marmish tooling is outside this migration.
- [ ] Regenerate the seven-crate workspace from its 111 pinned inputs and compare the entire emitted tree with the baseline. Replay 105 offline command-policy cases and 11 configuration-file cases on macOS ARM64 and publish the receipt. Baseline workspace lock SHA-256 is `2efe4cea5e65e5918024f7f79465c92297e47dc019a66399b4e5894eb6eb172c`. Live mixed-peer interoperability has separate acceptance tests.
