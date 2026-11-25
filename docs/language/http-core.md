# HTTP core, rule set 1

The demo contract identity is `urn:oddlang:contract:ledger-http`. Its operation IRIs are `urn:oddlang:ledger-http:LookupAccount` and `urn:oddlang:ledger-http:TransferFunds`. In the tables below, `h:` expands to `urn:oddlang:http:`. Unprefixed class and predicate names in the vocabulary table use that same namespace. Every declaration has one named RDF subject and one `rdf:type`. Identical triples have set semantics. The only ignored predicates are `rdfs:label` and `rdfs:comment`, including annotations on blank nodes. All other predicates and classes must be recognized. Target hints belong in separate project overlays.

## Vocabulary

Each property occurs once unless marked `?` (zero or one), `*` (zero or more), or `+` (one or more). Strings are `xsd:string`. Integers are `xsd:integer` within signed i64. References must be absolute named IRIs and resolve to the indicated class. Every normative declaration other than the contract header is an operation or reachable from an operation.

| Class | Predicates and ranges |
| --- | --- |
| Contract | `identity` absolute IRI, `version` nonempty ASCII letters/digits/hyphens, `ruleSetVersion` integer 1, `localImport*` relative Turtle file strings, `dependency*` exact version IRIs, `scenarioFile*` relative portable scenario file strings. One header per closure. |
| Operation | `method` string GET or POST, `path` route string, `request` and `response` Record, `status` integer 200, `role+` among `h:provider` and `h:consumer`, `error+` Error, `observation+` named observation, `behavior?` a pinned ledger Operation. Every guard error has an explicit HTTP mapping. |
| Record | `field*` Field, `constraint` Closed. Native and wire field names are unique within the record. Required record references are acyclic. |
| Field | `nativeName` ASCII letter then zero or more ASCII letters/digits/underscores, `wireName` nonempty exact string, `shape` one of `h:Text`, `h:Boolean`, `h:I64` or a named Record, `constraint+` exactly one Required and optionally one Bounds on I64 only. |
| Required, Closed | No properties beyond type. Represents member presence or object closure, respectively. |
| Bounds | `min`, `max` inclusive integers with min ≤ max. |
| Error | `status` integer from 400 to 599. Its subject is the wire error IRI. |
| LookupObservation, TransferObservation | No properties beyond type. Describe observable account reads and atomic transfer effects below. |

GET binds the sole request field (wire name `id`, Text) at exactly one `{id}` segment. POST encodes its request record as JSON. Route literals comprise nonempty segments of ASCII letters, digits or hyphens. A route starts with a slash. Method/path pairs are unique.

## Offline resolution

Run `oddlang http check-source SOURCE DEPENDENCY_LOCK CACHE` on a directory containing `domain.ttl`. Local imports and scenario files resolve relative to the referring Turtle file. A local import may name a sibling file or descend into a subdirectory of the referring file. Parent-directory imports are invalid even when both files lie within the source root. Repeated references and local import cycles are allowed. Read the closure offline and keep exact source bytes for publication. A source dependency lock holds only external pins. Its `pins` array contains objects with exactly `identity`, `version` and `manifestSha256`. Each dependency assertion uses `IDENTITY:VERSION`. An exact pin includes the 64 lowercase hex digits of SHA-256 over the manifest bytes. Conflicting pins for one identity fail.

Run `oddlang http check PROJECT WORKSPACE_LOCK CACHE` to resolve a published project import (`urn:oddlang:core:imports`) by identity or exact version IRI against a `pins` array. A release manifest lists its exact dependency pins, which must agree with any corresponding workspace entries. The legacy ledger identity resolves under `CACHE/ledger/VERSION`. HTTP identities resolve under `CACHE/http/HEX/VERSION`, where HEX is the lowercase SHA-256 of the UTF-8 identity. Legacy ledger project locks retain their single-object format.

An HTTP manifest has exactly `schemaVersion` (1), `identity`, `version`, `entry`, `ruleSetVersion` (1), `dependencies` (array of pins) and `files` (relative path to SHA-256). Verify the manifest against the workspace digest, verify every listed file before parsing Turtle, and require every source import and scenario to be listed. Reject unreachable ontology payloads. Relative paths use slash-separated normal components, exclude backslash, empty components, dot and dot-dot, and resolve within their root after following symlinks. Reordering equivalent RDF preserves semantic declarations. Source bytes and manifest digests retain exact-byte identity.

## Wire behavior and witnesses

The rule ID prefix in this table is `urn:oddlang:http:rule:1:`. Roles P and C denote `http.provider` and `http.consumer` (RDF values `h:provider` and `h:consumer`). Apply rules at each reachable operation/request/response field use. Providers receive requests and emit checked replies. Consumers emit requests and validate replies through an observable scripted peer. A request-side consumer rejection requires zero exchanges.

| Rule ID suffix | Roles | Required witness kinds |
| --- | --- | --- |
| route | P, C | Exact method and route, decoded ID containing escaped slash, space and non-ASCII text, literal plus, invalid percent escape or UTF-8 (P), empty or dot-segment IDs (C local rejection). |
| media | P, C | Matching JSON content type, case-insensitive and optional UTF-8 charset, rejected missing or unsupported media type, wrong response media (C). |
| json | P, C | Valid UTF-8, valid escapes, rejection of empty body, BOM, invalid UTF-8, isolated surrogate, malformed JSON, duplicate decoded member or trailing non-whitespace on received JSON. |
| record | P, C | Valid object and rejection of wrong JSON type or unknown member. |
| required | P, C | Member present and omitted. |
| text | P, C | Exact decoded Unicode scalar sequence and wrong type. No Unicode normalization. |
| boolean | P, C | Both Boolean values and wrong type. |
| i64 | P, C | Exact signed limits, zero, large exact integer, rejection of noncanonical decimal string, out-of-range value and numeric JSON token. |
| bounds | P, C | Both inclusive endpoints and rejection of each adjacent out-of-range value. |
| response | P, C | Success status, media and valid body. C distinguishes invalid response and transport failure from declared errors. |
| error | P, C | Exact named error and status. C rejects unexpected status, unknown error IRI or malformed envelope. |
| lookup | P, C | Existing account with maximum balance and missing ID. |
| transfer | P, C | Successful transfer, exact capacity, every named guard error and colliding guard failures. P checks state after success and failure. C checks request plus exact success or named error. |

Match routes by raw slash-separated path segments. Percent-decode a bound ID once, preserving literal plus. Reject invalid escapes, invalid UTF-8 and IDs equal to the empty string, dot or dot-dot. Clients reject those three IDs before dispatch. The declared exchange covers the two demo routes. Unmatched routes, undeclared methods, proxy limits, broken connections, host routing and framework-level transport failures are outside the named application responses.

Validate the matched path first, POST media type second, body third, then ledger guards. For POST require case-insensitive `application/json`, with no parameters or only a UTF-8 charset parameter. A missing or unsupported content type returns 415. Invalid path values, JSON or request schema return 400. Decode strict UTF-8 JSON and reject a BOM, isolated surrogates, empty bodies, non-whitespace after a value, duplicate member names after unescaping, and undeclared members. A decimal i64 is a JSON string matching `^(0|-?[1-9][0-9]*)$` and the inclusive signed range. Validate lexeme and bounds before conversion.

Lookup reads `GET /accounts/{id}`. An existing account yields HTTP 200 with id, owner and decimal-string balance. Missing yields 404 with `urn:oddlang:http:error:AccountMissing`. Transfer posts source, destination and decimal-string amount to `/transfers`, then returns both updated accounts under source and destination at status 200. Each error envelope has exactly one member, `error`, holding the full error IRI. HTTP-only errors use `urn:oddlang:http:error:InvalidRequest` (400) and `urn:oddlang:http:error:UnsupportedMediaType` (415).

The pinned ledger operation evaluates guards in priority order. SourceMissing and DestinationMissing map to 404, SameAccount and NonPositiveAmount to 422, and InsufficientFunds and DestinationOverflow to 409. All six use the `urn:oddlang:ledger:error:` prefix. With A=10, B=7 and amount=3, subsequent lookups return 7 and 10. On failure both balances and every unrelated account remain unchanged. AssignmentConflict and FieldOutOfRange are unreachable for this pinned operation because distinct IDs and both balance guards precede the buffered assignments. Any future reachable error needs a named mapping and case.

## Toolchain

Run `scripts/setup-http-conformance.sh` for pinned tooling. The baseline uses Rust 1.96.1, Go 1.26.3, Node 22.22.2, npm 10.9.7, TypeScript 5.9.3, Ajv 8.20.0 (JSON Schema 2020-12), the 2022-10-07 OpenAPI 3.1 schema, and Playwright 1.58.2 with its Chromium build. The hand-authored v1 cache fixture supports offline phase-one checks.
