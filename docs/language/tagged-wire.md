# Tagged wire records, version 1

The vocabulary `urn:oddlang:tagged-wire:v1:` describes one family of
length-delimited binary records. A contract is a finite set of distinct
unsigned byte tags. Every tag has a stable IRI and a native-facing name.
Input records supply one value per named field.

The wire stream comprises a tag byte, the payload length as an unsigned
32-bit big-endian integer, and the payload. The contract declares a positive
maximum payload length. This length counts payload bytes only. A named
variant has an ordered sequence of fields. A field can be a big-endian
unsigned integer of width 1, 2, 4 or 8, a fixed-width byte sequence, or a
bounded byte sequence that consumes the rest of the payload. Only the last
field can consume the rest.

Integer fields have a declared maximum, defaulting to the maximum of the wire
width. Fixed byte sequences specify their width. A bounded byte sequence has
an inclusive minimum and maximum. A bounded final field can also require
that its byte length plus an earlier integer field fit under that field's
maximum. The sum is mathematical, including at the maximum native integer.

On decoding, check the following in order.

1. The input contains the complete five-byte header.
2. The declared payload length is within the contract maximum.
3. Input length equals the header length plus the payload length.
4. The tag is declared within the contract.
5. Parse fields in their declared order. Check that enough bytes are
   available for each fixed-width field before reading it. Apply the local
   constraint immediately after reading each field. For the final field,
   check the byte length before the sum.
6. Reject remaining payload bytes.

The contract supplies a defined, named error for every step and field
constraint. Error IDs are opaque to the codec. An error value exposes the ID.
Encoding checks field constraints in declaration order before sending any
bytes. When the final field checks a sum, encoding defers the earlier
integer's range check to that sum, after the byte length check. The wire
width remains mandatory. It then checks the total payload length. A non-declared tag, wrong
native value type or missing/extra field is rejected as invalid input.

The target binding can choose native types and libraries. Unsigned 64-bit
values require a lossless representation within their full declared range
(for example, `u64` in Rust and `bigint` in TypeScript). The same byte stream
and errors are observable across bindings. A partial contract lists only
the variants it qualifies. Its use as a replacement for a larger protocol
requires a separate coverage claim.

Each record, field and error has an IRI. Positions are consecutive unsigned
integers starting at zero. A tagged wire graph contains only the declared
predicates and typed literals admitted by the loader. It carries no program
or embedded native implementation. A reviewed overlay may supply field
semantics still unavailable from automatic recovery. Reviewed and automatic
facts must have separate evidence.

## Graph predicates

Use `rdf:type` with object `t:Contract` for the root. The root has one
`t:maxPayload`, seven named error links (`t:shortHeader`, `t:payloadLimit`,
`t:lengthMismatch`, `t:unknownTag`, `t:truncated`, `t:trailing`,
`t:invalidValue`) and one or more `t:variant` links. Each variant has
`t:name`, `t:tag` and zero or more `t:field` links. Fields have
`t:name`, `t:position` and `t:kind`.

| Field kind | Additional predicates |
| --- | --- |
| `uint-be` | `t:width`, optional `t:max` and `t:rangeError` together |
| `fixed-bytes` | `t:width` |
| `rest-bytes` | `t:min`, `t:max`, `t:rangeError`, optional `t:sumWith` and `t:sumError` together |

`t:sumWith` links to an earlier ranged integer field in the same variant.
An error has one `t:code` string. Names and each slash-delimited error code
segment use ASCII letters, digits, hyphen or underscore, with a limit of
64 bytes per segment and at most two segments. Subject and object identifiers use bounded, named
URNs. Graphs have one root and all facts must be reachable and accounted
for by this vocabulary.

The graph pin hashes the unique RDF triples in lexical order, in N-Triples
format with a final LF on each line. Equivalent triple ordering and prefixes
preserve the pin. Role coverage and library hints can accompany the graph as
separate files. The tagged wire compiler's input is the normative graph
and its expected digest.

## Optional incremental decoding

Declare `t:feedFailed` and `t:incomplete` together on the graph root to
support an incremental decoder. A fresh decoder holds no bytes. A feed
buffers input until it has the complete header. At that point it checks
the payload length bound, then consumes only enough input to complete the
current record. Its success returns the number of bytes consumed and a
record only when one is complete. Successful delivery clears the buffer.
Callers feed any unconsumed bytes again to read subsequent records.

Any failed feed clears the buffered bytes and marks the decoder failed.
Subsequent feeds return `t:feedFailed` with no consumption. Finishing a
failed decoder or one with buffered bytes returns `t:incomplete`.
Finishing only observes state. It can report an incomplete record and
the caller can still supply its remaining bytes.
