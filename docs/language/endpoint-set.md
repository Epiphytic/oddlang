# Endpoint set normalization v1

The graph defines a pure ordered normalization operation over a sequence of
endpoint strings and a caller-supplied loopback preference. It returns the
canonical strings or the first named error. It performs no hostname lookup
or connection. A graph controls literal address policy and lexical host
validation. Resolved destinations and DNS rebinding are outside that
operation.

## Graph

The JSON graph has vocabulary `oddlang-endpoint-set-v1`, an absolute
`id` and a nonempty list of distinct absolute error identities under
`errors`. Every error used by a rule or the empty-set policy must appear
in that list. The digest ignores JSON member order and order within
membership sets, while preserving the order of the rules array. The parser identifier is `whatwg-url`. An optional
`providerHint` names a Cargo package and version for reproducible native
builds. The Rust target uses the third-party `url` crate.

`empty` is an `allow` or `reject` object, selected by `kind`. The reject
case names an `error`. It is evaluated before inspecting any input.
`deduplicate` selects whether to retain the first occurrence of every
canonical string or retain all occurrences. Either mode preserves input
order. A failure in any input discards the result.

Rules run in array order. A graph has one `parse` rule. Input rules precede
it and URL rules follow it. A `loopback-opt-in` rule requires a preceding
`host-policy` rule and may occur once. At most one host policy runs per
endpoint. A host policy requires an earlier allowed-schemes rule limited to
`ftp`, `http`, `https`, `ws` and `wss`, so WHATWG parsing classifies
numeric hosts as IP addresses. A host policy that declares loopback prefixes
or DNS suffixes requires the subsequent opt-in rule.

| Rule | Graph fields and operation |
| --- | --- |
| `forbidden-input` | `unicodeWhitespace`, `unicodeControl`, `characters` and `error`. Reject any matching raw character. |
| `parse` | `error`. Parse the input as an absolute WHATWG URL and report this error on parser failure. |
| `allowed-schemes` | `schemes` and `error`. Compare with the canonical scheme. |
| `url-components` | `forbid`, `allowedPaths` and `error`. Match normalized components `username`, `password`, `query`, `fragment` and `port`. A null or omitted path list allows any parsed path. An empty list allows none. |
| `host-policy` | `dns`, `ip` and `error`. Apply the declared policy to the parsed host. A missing or disallowed host yields this error. |
| `loopback-opt-in` | `error`. A classified loopback requires a true caller preference. |

DNS `syntax` can be `ascii-labels`, `any` or `deny`. After excluding one
terminal dot, the DNS name must be nonempty. ASCII labels are
nonempty, contain ASCII letters, digits or hyphens, and have no hyphen at
either end. A terminal dot is excluded from label checks and DNS suffix
classification. `trailingDot` can `preserve`, `strip-one` or `reject` the
dot. Strip-one resets the domain to its dot-stripped spelling. A setter
failure uses `rewriteError`. A canonical DNS name matching a
`loopbackSuffixes` member or its subdomain is classified as loopback.
The suffix comparison is ASCII case-insensitive and respects label boundaries.

The IP policy declares `loopback`, `ipv4` and `ipv6`. Loopback prefixes
take precedence and classify before each family's `deny` and `allow` lists.
The deny list takes precedence over the allow list. A literal outside those
lists follows `defaultAllow`. CIDRs use canonical textual network addresses
with all host bits zero. The IPv4 and IPv6 lists accept only their matching
address family. IPv4-mapped IPv6 literals follow the IPv6 policy. All lists
may be empty.

## Rust projection

`oddlang endpoint-set digest GRAPH` prints the digest of the typed graph.
`oddlang build endpoint-set rust GRAPH OUTPUT --graph-sha256 DIGEST` emits
`endpoint_set.rs` and `projection.lock`. The free operation is
`normalize(entries: &[String], allow_loopback: bool)` and returns
`Result<Vec<String>, &'static str>`.

An optional overlay has vocabulary `oddlang-endpoint-set-rust-v1`.
It declares `typeName`, `constructor`, `getter`, `errorType`,
`errorVariants` and `derives`. The checked type has a private string
vector. Its constructor has the same inputs as the free operation and
returns the checked type or the external error type. The getter borrows
the canonical `&[String]`. Unit-variant mappings must cover all graph errors.
The allowed derives are Clone, Debug, PartialEq and Eq.

```sh
oddlang endpoint-set overlay-digest OVERLAY
oddlang build endpoint-set rust GRAPH OVERLAY OUTPUT \
  --graph-sha256 GRAPH_DIGEST --overlay-sha256 OVERLAY_DIGEST
```

The projection deterministically embeds validated policy data. A native
consumer needs the URL provider dependency and, for a bound projection, its
declared external error enum. The native consumer does not read the graph
or reference application source during its build.
