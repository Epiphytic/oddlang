# Portable Marmish stream oracle

This suite exercises published Rust methods and stream, copy and codec
processing at revision `49fc705408a230bd0ad896248d08dbdd2faad05a`. The stream
package exports a Rust library. Executable CLI evidence belongs to the
[engine bridge suite](../portable-bridge/).

The independent comparator matched 50 prior cases, 96 additional authored
cases and 257 publicly seeded cases. The pinned original matched 128 sealed
cases and read 31 exact frames or envelopes emitted by the original. Rust
and a separate Python zlib peer exchanged the same payload using different
encoded bytes. The comparator rejected three one-field changes to expected
observations, covering the binary Data tag, error precedence and a window
state after rejection. [Original receipt](original-receipt.json)
· [Public observations](original-observations.json.gz)

## Provenance and coverage

| Corpus | Source and covered behavior |
| --- | --- |
| Prior | The archived vectors cover canonical wire JSON, malformed and overlapping copy frame errors, incremental decoder recovery or failure, compressed input, send-window consumption and receive retries. The historical caller also passed independently against this pin. [Vectors](../stream-vectors.json) · [Runner](../stream-driver/) |
| Authored | The language-neutral fixture preserves the independently authored bytes, hashes and canonical envelopes. Rust expands paired decoders and all 256 discriminants for each numbered enum. Cases cover every frame tag and JSON request and response variant in both directions, manifest and checkpoint encoding, helper context, launch bindings and digests, injectable time, codec block methods, public constants and the full u8 domain for numbered enum conversions. Expected error precedence comes from the original Rust guard ordering. [Fixture](authored-cases.json) · [Original layout derivations](https://github.com/Epiphytic/oddlang/blob/1fc9eb65398c6e530862576aad3848e99996541d/examples/marmish/portable-oracle/spec_cases.py) |
| Seeded | The public seed supplies Data offsets at zero, interior positions and the signed-64-bit ceiling, arbitrary binary payloads, input splits across headers and records, tag/length precedence and post-error observations. Receive cases track the complete public state across conflict, gap, bad acknowledgement, rejected EOF, exact retry and delivery. An additional case checks receipt capacity and eviction. [Seed generator](../../../verification/marmish-inventory/src/bin/portable-oracle/corpus.rs) |
| Holdout | An independent random 32-byte seed supplied 128 cases before target generation. Qualification owns the sealed input and expectations. The macOS sandbox denies the caller access to the capsule directory during sealed replay, and a direct read probe verifies that denial. Only case identifiers, operations and inputs reach the adapter. [Commitments](freeze.json) |

The upstream copy design describes the helper context, record encodings and
checkpoint layout. The upstream stream design specifies the limits, time
skew and zlib rules. [Copy layout](https://github.com/Epiphytic/marmish/blob/49fc705408a230bd0ad896248d08dbdd2faad05a/docs/phases/streaming-copy.md)
· [Stream design](https://github.com/Epiphytic/marmish/blob/49fc705408a230bd0ad896248d08dbdd2faad05a/docs/phases/streaming-primitives.md)

The original Rust guard ordering supplies the precedence expectations. These
include the four prior `*-before-*` frame cases, the seeded
`*-truncated-tag-precedence` cases, and five authored cases.
The authored cases are `exchange-ack-before-unclean-terminal`,
`manifest-offset-before-metadata`, `open-argv-before-age`,
`codec-block-length-before-offset` and `codec-block-offset-before-codec`. The holdout exercises
the same generator domains with a separate seed.

## Observation rules

- Canonical wire JSON, binary copy frames, checkpoints, raw decoded bytes
  and uncompressed codec blocks compare exactly. The driver represents
  binary bytes in hex. Only its observation envelope ignores object member
  order. Arrays and operation results retain their order. JSON integers
  must retain full precision. Public errors compare categories and named
  detail where the enum carries it.
- A compressed block compares exact codec, offset, raw length, SHA-256 and
  decoded bytes. A different encoding is allowed when it is shorter than
  the payload and is a single complete zlib stream with no preset dictionary
  or leftover input. The receipt preserves both encoders' bytes for a
  payload that both decoders read.
- Wire admission time is supplied as an input. Each scenario starts with
  public window and decoder defaults. The driver uses a disposable cwd and
  the recorded runtime environment, and expects exit status zero and empty
  stderr independently of protocol results.

## Replay

Use Rust 1.97.1 on Apple Silicon. Populate the offline
Cargo cache for the pinned upstream lock and use a clean upstream checkout.

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml \
  --bin portable-oracle -- \
  --upstream /path/to/pinned/marmish \
  --receipt /tmp/marmish-portable-oracle.json \
  --observations /tmp/marmish-portable-observations.json.gz
```

Qualification can add `--holdout /path/to/sealed/holdout-v1.json` from its
private copy. The public reproduction reports a null holdout result. The
receipt records compiler, target, features, upstream lock, caller and
dependency hashes, runtime environment and public observation digest. Binary
hashes include build paths. Reproduction compares the observation digest.

A separately built target can run the suite with
`--command /absolute/path/to/adapter` in place of `--upstream`. Add
`--peer-command /absolute/path/to/another-adapter` to exchange actual emitted
binary frames and canonical JSON envelopes in both directions. The adapter
reads one JSON array from stdin with `name`, `operation` and `input` keys
per row. It writes one JSON object with a `cases` array of `{name, actual}`
observations in the requested order. [Native caller](stream.rs)

The current Rust runner also checks the alternate encoding with system zlib. The frozen receipt and public observation archive retain their original tool identities. The Rust replay recovered the same public observation digest, alternate compressed bytes and three failing mutation identifiers. [Rust qualification](../../../verification/provider-tooling/portable-oracle-rust.json)

The generated Data qualifier compares both generated targets with the independent oracle and the original caller. It builds the approved graph with and without hints, retags a separate graph, checks direct-read denial at each build boundary, and exchanges actual packets between each pair of readers and writers.

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml \
  --bin qualify-tagged -- --oddlang /absolute/path/to/oddlang \
  --upstream /path/to/pinned/marmish --receipt /tmp/tagged.json
```

On macOS, add the private capsule with `--holdout` for sealed replay. For Linux public replay, specify the native `--original-target` triple. The Linux namespace requires bubblewrap. Where local user namespaces require privileged mount setup, `--linux-privileged-mounts` drops privileges before the caller runs. The qualification accepts blind input lines on stdin with `--external-judge` and emits the two observed output arrays on stdout.

## Remaining obligations

- The measured platform is Apple Silicon macOS, with the native development
  profile and locked features. Other targets, profiles and dependency choices
  remain open. Stream behavior with release overflow settings is unmeasured.
- Rust source compatibility needs unchanged downstream callers and complete
  path, signature, trait, auto-trait, visibility, feature, layout and
  dependency-type probes. This adapter covers the items it exercises. There
  is no standalone stream CLI or published C ABI.
- Nested coverage is one of four terminal variants, one of ten wire error
  codes, and one of six copy admission error variants.
- Untested combinations include guard overlap, offsets, malformed serde
  input, enum variants, decoder scheduling, compression representations
  and window transitions. Operational cases exercise public observations
  and sample the input domain.
- Recovered Rust and Node callers, alternative libraries in recovered
  builds, and higher-layer project behavior such as authentication,
  authorization, command execution, file transfer, durability, storage
  and installed tooling require separate evidence.
