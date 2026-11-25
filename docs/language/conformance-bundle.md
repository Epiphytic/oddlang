# HTTP conformance bundle, version 1

Build a release with `oddlang http publish SOURCE DEPENDENCY_LOCK CACHE EMPTY_OUTPUT`. Each release contains its original Turtle and authored scenario bytes, a case corpus, a witness inventory, JSON Schema 2020-12, OpenAPI 3.1.0, and a manifest giving the SHA-256 of every payload and the exact external dependency pins. The generated `workspace.lock.json` is a separate pin for the manifest. Copy the manifest and its listed files into the HTTP cache directory described in [HTTP core](http-core.md). Consumers pin those exact bytes. An equivalent Turtle rewrite can change manifest identity while preserving every derived projection.

## Obligations and applicability

`obligations.json` is an array. Each row identifies an operation IRI, role, declaration IRI, versioned rule IRI and position. Slots contain a witness kind and an ordered array of case IDs. Positions start at `request`, `response` or `behavior` and spell field uses with JSON Pointer escaping. Shared declarations occur separately at each use. Claims name operations and roles. The inventory walks each operation's records, fields, constraints, errors and observations. Publication requires every applicable slot to have a case. Unknown associations and duplicate case IDs fail.

A receiver exercises both valid and invalid wire inputs. Emitters demonstrate checked values and valid envelopes. GET request records bind to path segments, and have no JSON entity body. GET consumers reject empty, dot and dot-dot identifiers before dispatch. Providers return 400 for the empty trailing segment and for percent-encoded dot and dot-dot segments. POST providers return 415 when an unsupported media type and a malformed body appear together. Integer positives must satisfy the field's range. The v1 ledger requires a positive transfer between distinct accounts. Its successful provider response can have at most 9223372036854775806 remaining at the source and at least 1 at the destination. These two positions exercise their attainable bounds. Account lookup and scripted consumer replies exercise the full declared balance range. Raw decimal strings one step outside a bound can also exceed signed i64.

## Portable actions and observations

`cases.json` is sorted by `caseId`. An authored scenario file has the same array format. Each case contains `caseId`, `operation`, `role`, `slots`, `fixture`, `action` and `expected`. Each slot repeats the obligation's operation, role, declaration, rule and position and supplies its kind. A ledger fixture is a fresh array of accounts with string ID, owner and decimal-string balance.

A raw request has exactly `method`, `path`, `contentType` and `bodyBase64`. A raw response has `status`, `contentType` and `bodyBase64`. Missing media type is JSON null. Bodies use standard padded base64 of the original octets, including malformed input. An exchange observation has `request` and `response`. A transport failure records a dispatched request and a null response.

- For `http.provider`, the action's `requests` array contains ordered raw requests. Record both sides of every exchange. Transfer cases perform subsequent GETs for every fixture account, including the bystander.
- For `http.consumer`, the action has a native `input` record and a `replies` array for the scripted peer. Record the actual outbound request and the peer reply for each exchange, followed by one `native` observation. A successful native result has `kind: "success"` and a checked `value`. A named error has `kind: "namedError"` and its complete `error` IRI. A malformed or unexpected peer response or failed transport produces `kind: "protocolError"`. A local rejection has `kind: "localRejection"` and zero exchanges. Native i64 values serialize to canonical decimal strings.

## Claims and evidence

A claim file has `schemaVersion: 1` and a nonempty `claims` array of distinct `{operation, role}` pairs. The two role strings are `http.provider` and `http.consumer`. Evidence has `schemaVersion: 1`, `release` (identity, version and manifestSha256), `ruleSetVersion: 1`, `claims`, and `results`. Each result has `caseId`, `outcome` and ordered `observations` in the same exchange/native format as expected observations.

Run `oddlang http verify-evidence BUNDLE WORKSPACE_LOCK CLAIM EVIDENCE`. Verification checks the manifest and payload digests, validates claims, expands the claim through the inventory, and checks observed results against the bundle. An outcome of `pass` requires matching observations. Duplicate results, unexpected or unexecuted cases, stale pins, wrong rule sets, failed outcomes and mismatched observations fail. Evidence stays outside the release.

Compare methods, status, segment-wise percent-decoded paths, and media types. A slash inside a decoded segment is part of the ID. A plus remains a plus. Valid strict JSON compares by decoded value, retaining exact strings and ignoring member order and JSON whitespace. Malformed body bytes compare exactly. Empty and missing media types remain distinct.

## Schema projections

Each named record has a JSON Schema definition with a stable IRI key. The OpenAPI document embeds its own schemas and uses local fragment references. `x-decimal-range` records inclusive lower and upper endpoints as canonical decimal strings. Standard validators enforce the decimal lexeme and object structure. Native codecs and the portable cases enforce the numeric range.

### Standalone codec cases

Rule set 2 uses the `json.codec` role. A case supplies one wire body in `action.bodyBase64` (standard base64), an empty fixture array, and one expected native observation. A successful decoder reports `{"native":{"kind":"success","value":…}}` with the re-encoded JSON value. A decoder rejection reports `{"native":{"kind":"localRejection"}}`. The verifier compares decoded JSON values, so member ordering and escape spelling are immaterial. Shape positions use JSON Pointer segments following `body`. The obligation position for a union branch appends its tag with an `@` prefix. List item witnesses use sample index `0`.

The shape profile derives shape, closed-object, presence, nullable, element, and tag witnesses from the model. Native Rust, TypeScript, and Go runners execute the same bytes. Running these witnesses makes a codec claim. HTTP route and transport coverage continues to use provider and consumer claims under rule set 1.
