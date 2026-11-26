# Positioned integrity blocks

An integrity block carries bounded byte content at an unsigned offset, the decoded length, a representation tag and a digest of the decoded bytes. Integrity checks identify damaged content. Transport authentication and authorization remain the caller's responsibility.

```sh
oddlang build integrity-block rust GRAPH_JSON RUST_BINDING_JSON OUTPUT
```

The generator publishes `codec.rs`, `obligations.json` and `projection.lock`. The lock pins the canonical parsed graph, Rust binding, generated source and language-neutral obligations. Each obligation names a semantic declaration and an expected effect or rejection. Projection marks every witness as independently required.

## Semantic graph

The JSON vocabulary is `oddlang-integrity-block-v1`. Its `id` and named errors are absolute IRIs. `fields` contains exactly one declaration for each semantic role, with a distinct nonempty `wireName`.

| Role | Meaning |
| --- | --- |
| `offset` | Unsigned 64-bit starting position |
| `representation` | Named identity or compressed representation |
| `rawLength` | Unsigned 32-bit decoded byte count |
| `digest` | SHA-256 of decoded bytes |
| `payload` | Encoded bytes |

The declared field order controls object emission and positional tuple decoding. `wire.shape` is `closed-object-or-tuple`. A record requires exactly the declared fields, and rejects duplicate or unknown object members and short or long tuples. `wire.digest` is `lower-hex` and `wire.digestSyntaxMessage` describes malformed digest text. `wire.payload` is `base64-standard-padded`. The payload parser checks the encoded text length before allocating decoded content, then checks the decoded length and canonical representation.

`bounds` supplies inclusive `rawMinimum`, `rawMaximum`, `encodedMinimum`, `encodedMaximum` and `offsetMaximum`. End position uses checked addition and is inclusive at the maximum. The profile reserves space for a one-byte decompression sentinel and a portable Base64 length. It rejects bounds beyond those arithmetic limits. The offset limit remains an exact unsigned integer.

The required digest primitive is `sha256`. `representations.identity` supplies its wire label. An optional compressed declaration specifies `wireName`, `algorithm: "zlib"`, `effort: "default"`, `selection: "strictly-smaller"`, `members: "exactly-one"`, `dictionary: "forbidden"` and `trailing: "forbidden"`. Compression requires explicit negotiation, a strict size benefit and a result within the encoded bounds. Otherwise encoding retains the identity representation. An identity-only graph emits no compressor. Compressor output can vary by provider, while both peers check member structure, length and decoded digest.

Encoding checks raw input size before constructing and validating the identity block. `validateOrder` orders the four checks `raw-size`, `encoded-size`, `offset` and `representation-size`. Identity payload length must equal raw length and compressed payload length must be strictly smaller. Decode validates the block before executing `decodeOrder`, currently `["negotiated","dictionary","member","decoded-size","digest"]`. It checks for an exact zlib member and exact decoded length before comparing the digest. Invalid content releases no partial result.

`errors` assigns stable identities to `size`, `offset`, `representation`, `payloadSyntax` and `payloadDecodedSize`. Multiple roles can share one identity. `errorMessages` optionally assigns normative serde diagnostic strings by identity. If an identity has no message, its external Rust error supplies the diagnostic and this remains a target binding obligation. Optional `providerHints` are descriptive and have no effect on emitted code or obligations.

## Native Rust binding

The binding vocabulary is `oddlang-integrity-block-rust-v1`. It supplies `blockType`, `representationType`, field names keyed by role, the Rust `variants` for identity and optional compression, and `operations` for `end`, `validate`, `encode` and `decode`. It binds an external `errorType`. Every declared error IRI has an entry in `errorConstructors` with a Rust `variant` and optional literal string `argument`. An optional `payloadHelper` supplies a crate-visible module name and decode operation for callers of the bounded Base64 primitive.

The native target uses Serde, Base64, SHA-2 and optional flate2. The independently authored terrain fixture checks exact wire bytes, both decoding representations, size and offset precedence, malformed data and a real preset-dictionary member. [Qualification command and fixture](../../verification/integrity-block/README.md)
