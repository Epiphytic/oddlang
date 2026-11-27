# Workspace providers

Use `oddlang provider run ROLE CAPABILITY VERSION REGISTRY INPUT OUTPUT` to invoke a registered component. The output directory must be new. This generic command requires an explicit registration and applies the envelope and registered document schema. The profile-specific build and recovery commands also validate the public-record model, projection and source selection. The generic command stages the guest's proposed files and publishes them together with `provider-receipt.json`. Use the existing native workspace build command when you also want Cargo to compile the result.

A registry pins the component and the schema of the request's `input` value. Paths are relative to the registry directory. Specify one registration per exact role, capability and contract version. When a capability has registered versions, requesting another version is an error. With no override the compiler uses its native implementation for capabilities it ships.

```json
{
  "format": "oddlang-provider-registry-v1",
  "providers": [{
    "role": "generate",
    "capability": "example.station",
    "version": "1",
    "world": "oddlang:provider/generate@1.0.0",
    "schema": {"path": "station-schema.json", "sha256": "<64 lowercase hex digits>"},
    "component": {"path": "station.wasm", "sha256": "<64 lowercase hex digits>"},
    "configuration": {}
  }]
}
```

The [published WIT](../../contracts/providers/provider.wit) defines the import-free worlds and their typed byte-list functions. Both exchange UTF-8 JSON. The host serializes a request with `format` equal to `oddlang-provider-input-v1`, the selected `role`, `capability`, `version`, `configuration` and the `input` document. The public-records generator receives a graph, effective overlay and Serde availability. Its recovery counterpart receives a selection and a list of named, base64-encoded source files.

Each schema node requires a `type` of object, array, string, integer, number, boolean or null. Omit unused keywords. Each supplied keyword must have its declared type, including in nested schemas. Object nodes can specify `properties`, `required` and a boolean `additionalProperties`. Array nodes can specify an `items` schema. The host rejects unknown schema keywords, duplicate keys and schema nesting beyond 32 levels.

A successful response has this form. Artifact contents use standard padded base64.

```json
{
  "format": "oddlang-provider-output-v1",
  "artifacts": [{"path": "module.rs", "base64": "Cg=="}],
  "exports": [],
  "dependencies": [],
  "evidence": [],
  "gaps": []
}
```

To report a failure, return `{"format":"oddlang-provider-diagnostics-v1","diagnostics":[{"code":"unknown-type","path":"source.rs","message":"Expected an unsigned integer field"}]}`. The host reports the diagnostics and leaves the destination absent.

Set `providerRegistry` in a native workspace manifest to a registry pin such as `{"path":"registry.json","sha256":"<hash of registry bytes>"}`. Public-record components then use the selected generator. To compose a new capability into a Rust library, use a package component with `"kind":"provider"`, `"provider":{"capability":"example.station","version":"1"}` and the usual pinned graph, overlay, module and reexports. This capability receives `{"graph":...,"overlay":...}` and emits one Rust file named `module.rs`. The host checks its declared exports and dependencies against the package, and checks its Rust syntax. Existing build commands check Cargo metadata and binary targets where applicable. Run Cargo tests against the generated library to verify its behavior. Standalone runs allow zero dependency requests.

The versioned receipt identifies the provider, its inputs and the SHA-256 of each proposed artifact. Workspace packages record per-component receipts; historical builds without a registry retain their original lock and file layout. The host grants no WASI capabilities. Its limits include a 16 MiB component, 8 MiB request and result documents, 256 artifacts, 32 MiB guest memory and 50 million Wasmtime fuel units. Fuel applies to Wasm execution. Wasmtime compiles the size-limited component before instantiation. Cargo build scripts and executables run under the caller's own build sandbox.

### Grounded identifiers

Following the [OntoGPT template-authoring guide](../../skills/ontogpt-author-template/SKILL.md), declare which fields carry entity identities. A provider schema can require an absolute RDF IRI with `{"type":"string","format":"iri","iriNamespaces":["urn:example:"]}`. The optional list uses expanded namespaces ending in a colon, slash or hash. Values need a nonempty local part when namespaces are specified. A plain string field preserves free text. The check is deterministic and runs locally before the guest receives the document.
