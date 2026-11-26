# Ordered JSON wire schemas

The `oddlang-json-wire-schema-v1` graph describes named closed records, unit enums and adjacent tagged unions. Stable IRIs identify definitions, fields, cases and validation guards. The `oddlang-json-wire-schema-rust-v1` binding supplies native names, derives and external Rust paths. Text enum contracts, compression blocks and other separately qualified wire types can appear as named external references.

```sh
oddlang recover wire-schema SOURCE_RS SELECTION_JSON OBSERVED_DIR
oddlang build wire-schema rust GRAPH_JSON RUST_BINDING_JSON OUTPUT_DIR
```

The recover command checks the source bytes against `sourceSha256` in the selection. It reads selected top-level Rust declarations and recovers ordered fields, named types and vetted Serde attributes. The profile understands explicit `rename`, enum `rename_all = "snake_case"`, `tag`, `content`, `deny_unknown_fields`, and declared `with` adapter identities for fixed lowercase hexadecimal bytes and bounded Base64 byte lists. Adapter policy comes from the selection and carries an explicit independent-witness gap. Conditional declarations, unvetted attributes, shadowed primitive names and unresolved type references become explicit structural gaps. Derive recovery verifies the visible Serde import or qualified path. Structural candidates also list validation behavior as unobserved. The observation pins source, selection, graph and native binding by SHA-256.

The build command consumes only the graph and binding. It rejects unresolved shapes or source evidence gaps, invalid type references, recursion, binding collisions and ill-typed guard operands. It writes `wire.rs`, `obligations.json` and a lock covering both outputs and their inputs. Externally supplied types remain separate qualification dependencies. Native derives require checked support through nested types. An external Rust binding declares its witnessed derives under `externalDerives`. The generator relies on the native Serde provider to implement the declared shape.

## Shape and guard semantics

- Records emit an object with fields in declared order. Serde also accepts positional tuples for named structs. An optional value accepts a missing key and JSON null, and the native projection emits its key and null when that value is absent.
- Adjacent unions declare separate tag and content keys. Each case has an exact wire name and declares a unit, one named value, or an ordered record payload. Unit cases emit the tag without content. Closed shapes reject unknown and duplicate object fields.
- Field values can be booleans, text, unsigned 8-, 16-, 32- or 64-bit integers, optional values, ordered lists, named references, or a declared byte adapter on a named field. The Base64-list adapter receives item, encoded-byte, aggregate-byte and per-item accounting rules. It validates list size, encoded text length, decoding and canonical padding, and total decoded bytes in order. Overall frame bounds belong to the caller before parsing.
- A `unit-enum` declares stable variant IDs and wire names. Rust accepts a known string or a single-key object whose key is that name and whose value is null. It emits the string. Existing `external-tag-u8` hash slots use their external type and numeric tag table; keep those bindings pinned when reproducing historical digest bytes.
- A nullable member with `requirePresence` must be present and can hold null. Recovery recognizes the fully qualified Serde deserializer and explicitly selected `required-nullable` adapters. Other custom deserializers need independent structural evidence.
- A validation guard selects a record or a named record case, optionally checks a `when` predicate, and requires a typed predicate or returns its stable error IRI. Guards run in declaration order. The initial profile supports unsigned ranges, boolean and presence checks, list counts and uniqueness, and unsigned field comparisons. A guard applies only to its declared owner and case. Validation of a referenced value is a separate operation.
- The emitted validator is independent of Serde admission. Byte-canonical envelope validation, input-to-envelope mappings, typed foreign validation calls and time-sensitive checks require additional declarations and independent evidence.

The locked Rust 2021 fixture compares original and generated wire bytes, rejects malformed records and codecs, and changes a simultaneous-failure result by moving one guard. It also detects a changed source pin, an unverified Serde attribute and an ill-typed guard. [Verification](../../verification/wire-schema/verify.py)

A `bounded-base64` field maps one byte vector to standard padded Base64. Its `policy` specifies `maxBytes`, `encodedLimitMessage`, `decodedLimitMessage` and `syntaxMessage`. Parsing checks encoded length, decoding, decoded length and canonical re-encoding in that order. Encoding represents the supplied bytes; semantic guards run separately. `count` applies to the decoded byte count and `excludes-byte` forbids one byte in a scalar or fixed byte field.

A Rust binding can specify `nativeValidation` per guard owner, with a native `method`, error `typePath`, and a map from every guard error IRI to its native `variant` and optional static text `argument`. This replaces the default IRI-returning method for that owner. The graph still determines which guard runs first. The unrelated scalar-byte test exercises the typed API, checksum-independent wire rules and validation separately. [Scalar-byte fixture](../../verification/wire-schema/byte_value.py)

## Typed hash views

A record can declare named `hashViews`. Each view supplies its stable `id`, record `owner`, `algorithm` (`sha256`), a byte array `domain`, an ordered list of `fields` slots, and an `output`. The hash input begins with the domain bytes, followed by the encoded slots in the order given. A slot selects a field IRI or a parameter IRI and one encoding.

| Encoding | Input | Bytes |
| --- | --- | --- |
| `unsigned-be` | Unsigned integer | Big-endian bytes at the declared width |
| `bool-u8` | Boolean | Zero for false and one for true |
| `fixed-bytes` | Fixed lowercase-hex wire field | Raw fixed-width byte array |
| `u32-be-length-prefixed-bytes` | Scalar Base64 wire field | Big-endian 32-bit byte length and raw bytes |
| `external-tag-u8` | External enum field or parameter | Byte assigned by the external type's `hashTags` table |

The optional `parameters` array supplies ordered `{id, external}` declarations for native inputs. A `hashTags` map keys each table by external enum IRI, and the table keys its distinct byte tags by variant IRI. The Rust binding's `hashVariants` maps those variant IRIs to native enum constructors. Exhaustive native matches require a binding for every variant. Project owners qualify numeric assignments independently of their binding.

A view with `validate: true` runs its owner's normal validator first and returns its first error. Length-prefixed byte slots require that validation and an unconditional count guard to bound directly constructed values before the length conversion. The `bytes` output returns the digest array. The `lower-hex` output returns its literal `prefix` followed by 64 lowercase hexadecimal digits.

The Rust binding's `hashViews` map supplies each view's `method`, `library` (`sha2`), and a map of parameter IRIs to native names when the view takes arguments. The native package pins the selected provider. The projection derives independent-witness obligations for each view and each numeric tag. View declaration order is canonicalized by IRI. Slot order and parameter order remain observable. [Independent byte-level fixture](../../verification/wire-schema/hash_views.py)
