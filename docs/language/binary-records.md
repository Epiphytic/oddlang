# Ordered bounded binary records

An `oddlang-binary-record-v1` graph declares one record with a stable `id`, a total byte `maximum`, an ordered `layout`, and named errors. Each layout slot has a stable `id` and one closed `encoding`.

| Encoding | Native value | Byte layout |
| --- | --- | --- |
| `literal` | No field | Exact `bytes` |
| `length-prefixed-literal` | No field | Big-endian u32 length, then exact `bytes` |
| `unsigned-be` | Unsigned integer | 8, 16, 32, or 64 bits in big-endian order |
| `fixed-bytes` | Fixed byte array | Exactly `size` bytes |
| `bytes` | Byte vector | Big-endian u32 length, then uninterpreted bytes |
| `ascii-atom` | String | Big-endian u32 byte length, then ASCII letters, digits and the selected ASCII `punctuation` bytes |
| `enum-u8` | External enum | One byte from a stable variant IRI to byte `tags` table |

Unsigned integers, byte vectors and ASCII atom fields declare inclusive `minimum` and `maximum` bounds. A length-prefixed literal separately declares a read `maximum`.

Decode checks the aggregate limit, then reads slots in declaration order. A fixed literal is matched as a prefix and uses the slot's error even when the input is shorter than that prefix. Fixed-width field truncation uses the record's `truncated` error. Length-prefixed slots first read the four-byte length, reject a length over the slot maximum with `overflow`, then read the payload. An atom's alphabet and minimum length, a length-prefixed literal's equality, and an enum tag are checked at the slot and return their respective `error`. The parser rejects trailing bytes before applying record validation. Record validation checks both bounds on each byte vector and all native integer and atom constraints. It reports the record's `value` error.

Encode validates the native record first, computes its exact length and rejects excess before allocating the output. `decodeSize` and `encodeSize` select the aggregate errors independently. The profile covers fixed ordered layouts and one-byte enum fields. Optional payloads, tagged unions and cross-record constraints need separate declarations.

An `oddlang-binary-record-rust-v1` overlay binds the public record `name`, field `names`, external enum paths and variant names, and each named error to a Rust `errorType`, variant and optional text argument. Paths are absolute through `crate::` or a leading `::`. The resulting struct exposes checked `validate`, `encode` and `decode` methods. The build emits `record.rs`, declaration-keyed `obligations.json` and `projection.lock`. A pinned native package can use the `binary-record` component kind. [Independent fixture](../../verification/binary-record/verify.py)

```sh
oddlang build binary-record rust graph.json rust.json output
python3 verification/binary-record/verify.py target/debug/oddlang
```

## Binary metadata

A `bool` slot uses one byte, with zero and one as the only accepted tags. An `optional` slot uses the same tags for absence and presence, followed on presence by its declared `payload`. Both declare a slot error for other tags. An optional payload can be an unsigned integer or a named inline `record`. Inline records declare ordered fixed-width integer members and can mix signed and unsigned fields. Signed fields use two's-complement big-endian bytes. Their Rust binding supplies the type and member names.

An unsigned slot may supply a `readError` to check its range immediately after reading the integer. Integer and byte slots can supply `valueError` to override the default native validation error. A byte slot can exclude specified bytes. Native validation checks these field constraints in layout order, then applies the declared `guards` in order. The decoder applies native validation after its trailing-byte check.

A `presence-equals` guard equates a boolean with optional presence. An `optional-mask` guard requires all `required` bits and forbids bits outside `allowed` when an unsigned value is present. An `optional-member-range` guard applies an inclusive unsigned range to a member of a present inline record. Each guard has a stable identity and a named validation error.

Binary `hashViews` select an ordered list of fixed byte arrays and enumerated tag fields. The digest is SHA-256 of the declared domain bytes and selected wire representations. The output is the supplied prefix followed by lowercase hexadecimal digest bytes. The corresponding native binding selects the method name and SHA-256 library. These views operate independently of validation. [Independent metadata fixture](../../verification/binary-record/metadata.py)

## Composite binary records

A `nested` slot stores a checked external record with a big-endian u32 byte length. The slot declares its bound and overflow error. Decode delegates to the named record at that point in the layout and propagates its error. Validation checks each nested record when its field is reached. The graph declares any external members inspected by constraints, and the Rust overlay binds those names to the projected type.

A `signed-be` slot stores a signed fixed-width integer. An `inline` slot stores the ordered integer members of a required declared record without a presence tag. Setting `requireFull` on a literal checks truncation before comparing the complete literal.

A composite constraint compares typed local fields, declared nested fields or exact literals. It can require enum membership or optional presence, compare a present inline record with another field, impose a checked minimum signed time difference, and combine predicates by implication and equivalence. An overflowing time difference fails the constraint. Constraints run in order after the field guards and named record guards.

Optional `integrity` specifies a SHA-256 checksum over explicit domain bytes and the record payload. The encoded footer is 32 bytes and counts toward the complete byte bound. Decode first checks the maximum and the presence of the full footer, then compares the digest, then parses the payload. The graph supplies separate errors for short and mismatching footers. [Independent composite fixture](../../verification/binary-record/composite.py)
