# Bounded sequence journal v1

A journal retains an ordered byte interval with an acknowledged start cursor,
an appended end cursor, and optionally a final cursor. A separate bounded
receipt sequence can remember prior submissions by start, end and SHA-256
digest. Trimming payload bytes leaves receipts available until a declared
transition evicts one. This is an in-memory contract. No append or receipt
implies durable storage or an external delivery effect.

The JSON graph has format `oddlang-sequence-journal-v1`. Its members are
`limits`, `errors`, `states` and `operations`. State, operation, case and
error identities are distinct absolute IRIs within their respective roles.
Every operation names a state. Host observations and effects execute
according to the declared rules.

## Types and limits

The value types are `cursor`, `size`, `flag`, `optional-cursor`, `bytes`
and `unit`. Cursor arithmetic is unsigned and checked against the state
cursor limit. Size literals and configured size limits fit an unsigned
32-bit integer. Native size parameters may span the Rust machine word.

`limits` maps each name to `{"cursor": N}` or `{"size": N}`. Cursor values
can use canonical unsigned decimal strings to preserve full precision in
JSON consumers. The compiler normalizes either cursor spelling to a decimal
string when hashing the typed graph. Each state declares `id`,
`byteCapacity`, `recordCapacity`, `cursorLimit`, `appendLimit` and
`receiptCapacity`. The latter five fields are names in `limits`.
`initialCursor` specifies the shared start and end of the empty state. A
zero receipt capacity disables receipt storage.

Every record consumes positive bytes, respects the append limit and fits
the retained byte and record capacities. A partially trimmed record counts
as one record until fully consumed. The final cursor, when present, equals
the end. Appending to a finalized journal fails. Receipt records have
nonoverlapping start/end positions in submission order. Digest calculation
uses the pinned target's SHA-256 implementation. Receipt capacity and byte
journal capacity are independent.

## Ordered operation cases

Each operation has `id`, `state`, `access` (`read` or `write`), ordered
`params` entries `{name,type}`, `result`, `fallible` and ordered `cases`.
A case declares `id` and a list `when` of flag expressions. The first case
whose conditions all hold wins. Exactly the last case has no conditions.
A winning case either declares `reject` with a stable error IRI, or
`value` and ordered `effects`. Read operations have no effects.
Infallible operations can neither reject nor select an effect.

All expressions are evaluated against the original state of the operation.
A selected successful case applies its prepared effects in order to a
staged state. Failure of an effect discards the whole staged state and the
provisional result and returns the effect's named error. Rejected and read
cases leave the state unchanged.

| Expression | Result |
| --- | --- |
| `{"arg":"name"}` or `{"limit":"name"}` | Declared parameter or limit type |
| `{"cursor":N}`, `{"size":N}`, `{"flag":true|false}`, `{"none":true}`, `{"unit":true}` | Typed literals |
| `{"fact":"start|end"}` | Cursor |
| `{"fact":"final|front-receipt-end|last-receipt-start|last-receipt-end"}` | Optional cursor |
| `{"fact":"buffered|byte-credit|record-count|record-credit|receipt-count|receipt-credit"}` | Size |
| `size-of(bytes)` | Size |
| `advance(cursor,size)`, `some(cursor)` | Optional cursor |
| `exists(optional-cursor)` | Flag |
| `equal(A,A)` | Flag for cursor, size, flag or optional cursor |
| `less(A,A)`, `greater(A,A)` | Flag for matching numeric operands |
| `and(flags...)`, `or(flags...)`, `not(flag)` | Flag |
| `choose(flag,A,A)` | Common branch type |
| `slice(cursor,size)` | Owned bytes starting at the cursor clamped to the retained interval, up to the requested size |
| `has-receipt(cursor)`, `same-receipt(cursor,optional-cursor,bytes)` | Flag |
| `front-receipt-evictable()` | Flag, true when the oldest receipt ends at or before the start cursor |

The optional `expressions` map defines named expressions. An
`{"expr":"name"}` reference is evaluated at its point of use. Definitions
can refer to parameters of the using operation and are type-checked in
that operation. Every referenced definition must exist, and cycles are
rejected even when unused.

Function-like entries use `{"op":"name","args":[...]}`.
`byte-credit` measures remaining byte capacity. `record-credit` measures
remaining chunks. A projection can declare usable credit by combining these
facts with finalization. This choice also belongs in guards when it affects
error precedence. `same-receipt` requires an exact starting cursor,
calculated ending cursor and SHA-256 digest. Missing or conflicting receipts
have separate predicates.

| Effect kind | Typed arguments and transition |
| --- | --- |
| `append` | `bytes` appends one record at the current end |
| `trim` | `cursor` consumes the retained prefix through that cursor |
| `finish` | `cursor` marks the current end, with repeating the same marker allowed |
| `evict-receipt` | Removes the oldest retained receipt |
| `remember` | `cursor` and `bytes` give the bounds and digest of a new receipt |

Each effect also requires `error` naming the failure if its storage
preconditions are unsatisfied. The graph declares admission, retry match
outcomes, retention policy, any earlier guards and finalization/credit
interactions.

## Rust projection

The overlay has format `oddlang-sequence-journal-rust-v1`. It maps state
IRIs through `types` and operation IRIs through `methods`. It supplies
`errorType` as a Rust path and maps graph errors through `errorVariants`
to variants of that external enum. Every state and operation is bound
once. Each state type implements `Default` with its declared initial
state. Method borrows follow `access`. Cursor and size map to `u64` and
`usize`. Bytes enter as `&[u8]` and leave as `Vec<u8>`.

```sh
oddlang sequence-journal digest GRAPH
oddlang sequence-journal overlay-digest OVERLAY
oddlang build sequence-journal rust GRAPH OVERLAY OUTPUT \
  --graph-sha256 GRAPH_DIGEST --overlay-sha256 OVERLAY_DIGEST
```

The Rust projection uses the `sha2` crate for fingerprints. Supply its
locked version as a native package dependency. Component kind
`sequence-journal` composes with other native package components.
The projection lock pins generated source and `obligations.json`.
The latter enumerates ordered case identities per operation, named
rejections and effect failures that must leave state unchanged. A
portable scenario is still required to demonstrate each case.

## Native storage scope

The checked type uses one authoritative private journal store for its
cursors, byte records, final marker and receipts. This version does not
bind private fields individually. Compatibility investigations that depend
on native field names, private receipt container inspection or direct
cursor-field struct construction therefore need a separate storage
binding and qualification.
