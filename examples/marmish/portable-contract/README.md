# Copy Data contract

The contract applies to one complete `Frame::Data` record from Marmish
revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. It describes that record
and its envelope errors for malformed headers, malformed Data payloads and
tags unassigned upstream. The shared incremental envelope applies to the
same Data slice. Other copy variants, window state and the bridge process
remain outside this graph.

The extractor observed tag 5 in the encoder and decoder arms and two
encoder writes in order. The offset occupies eight big-endian bytes and the
byte vector follows without a second length. The source review accounts for
decoder consumption, limits, error names and precedence. The portable
invalid-input error comes from the target contract. These findings and their
source content digests are recorded separately from the normative graph.
[Candidate](candidate.json) · [Provenance](provenance.json) ·
[Reviewed overlay](data-overlay.ttl) · [Graph](data.ttl)

On encode, a byte count outside 1–8192 returns `Limit` before checking the
sum. On decode, a truncated offset fails before the range check and an
out-of-range offset returns `Offset` before the byte count is checked. In
both directions, a valid byte count plus the offset must fit within
`9223372036854775807`. The upstream reader defines the trailing-byte error
and the selected variant consumes the whole remainder on success. A feed
failure makes further feeds return `Invalid/copy_decoder_failed`. Finish
reports `Invalid/copy_truncated` after a failure or while a record is incomplete.

Independent upstream evidence includes the authored `spec-frame-data-encode`
and `spec-frame-data-decode` cases and the seeded Data reads, writes and offset
overflow cases. Both generated modules passed the scoped downstream replay. [Qualification receipt](../portable-oracle/tagged-qualification.json)
[Authored cases](../portable-oracle/authored-cases.json) ·
[Seeded cases](../../../verification/marmish-inventory/src/bin/portable-oracle/corpus.rs) ·
[Original receipt](../portable-oracle/original-receipt.json)

The separately pinned advisory hints describe the Rust byte buffer and the
locked compression library used by the wider stream package. The graph
build reads the contract alone. The zlib observation has no qualification
claim. [Hints](hints.json) · [Hint pin](hints.sha256)

From the repository root, build and recheck with a checkout at the pinned
upstream revision.

```sh
cargo build --locked --offline
cargo test --locked --offline --test tagged_recovery
cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml \
  --bin tagged-candidate -- \
  target/debug/oddlang /path/to/pinned/marmish
target/debug/oddlang tagged-wire digest examples/marmish/portable-contract/data.ttl
```

The replay compares the candidate to the pinned source observations and
reproduces the graph from the reviewed overlay and recovered tag. It changes
tag 5 to 51 in a temporary copy of both Rust arms and checks that only the tag
changes in the graph. Editing one arm is rejected before graph publication.
A private writer rename preserves the observations and graph digest. The
unrelated test fixture writes a four-byte integer before a byte vector and
declares those fields in the opposite order. Build-resolved semantics,
automatic materialization of field observations, bounds and error extraction, and obligation coverage for the wider project remain open.
