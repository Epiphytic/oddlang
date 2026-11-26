# Portable copy Data frame

Oddlang recovers the Rust declarations and emits a candidate structural
ontology. A separate contract describes the public copy Data record and
incremental decoder semantics. The record carries a one-byte tag, a
big-endian length, a big-endian offset, and a bounded byte payload.

The Rust reader and writer each supply the observed Data tag. A reviewed
overlay supplies the byte bounds, field semantics, guard order and errors
that require further interpretation. From the pinned checkout, combine the
observed tag with the overlay and compare the resulting graph digest
against the published digest. [Tagged wire vocabulary](../language/tagged-wire.md)
· [Marmish facts and provenance](../../examples/marmish/portable-contract/)

From the repository root, build Oddlang and the two native modules.

```sh
cargo build --locked --offline
wire=examples/marmish/portable-contract/data.ttl
pin=82ddfed871221091be6b42adf6d0b009cb5ce10f864c414bdbaed5e19e4dad56
out=$(mktemp -d)
target/debug/oddlang build tagged-wire rust "$wire" "$out/rust" --graph-sha256 "$pin"
target/debug/oddlang build tagged-wire typescript "$wire" "$out/node" --graph-sha256 "$pin"
rustc --edition=2024 --crate-type lib "$out/rust/wire.rs" --out-dir "$out/rust"
node --experimental-strip-types "$out/node/wire.mts"
```

The contract pin binds the named semantic graph. Each output records that pin
and its target. Rust emits a standard-library module. TypeScript uses native
`bigint`, `Uint8Array` and `DataView`. The tested slice covers encoding, decoding and
the declared incremental state, including input fragmentation, consumed-byte
counts and failure behavior.

The qualification driver stages the executable, the portable graph and
its pin for projection. The compiler runs inside an isolated directory with
the native toolchain and generated files. Read probes cover upstream Rust,
the private structure, the oracle and the sealed inputs. The two targets matched the original on 253 public and 112 sealed cases. All six directed writer and reader combinations cross-decoded 36 exact canonical Data frames.
The verification record includes scoped case counts and a retagged witness. One unchanged encode request produces tag 51 in both mutants, and their one-shot and incremental decoders accept the retagged frame and reject tag 5. [Qualification receipt](../../examples/marmish/portable-oracle/tagged-qualification.json) · [Companion contract PR](https://github.com/Epiphytic/marmish-oddlang/pull/3)

| Surface | Evidence and remaining work |
| --- | --- |
| Data-frame encoder and decoder | Public, seeded and sealed cases, error ordering, full-width offset boundaries and mixed wire peers within the Data role |
| Incremental decoding | Data record fragmentation, multiple records in one input, EOF with partial input and sticky failure |
| Compiler structure | Candidate inventory for three pinned targets. Exact declarations and relationships still require reconciliation. |
| Stream Rust API and remaining processing | Other variants, public Rust paths and traits, canonical JSON, stream windows, checkpoints and zlib role bindings remain open. |
| Bridge CLI and broader project | Original bridge baseline established. Recovered commands, SQLCipher and worker behaviors, platform effects and the remaining workspace remain open. |

The portability result applies to the named binary Data role on the
recorded profile. The native modules expose their own type signatures.
The original library's full Rust source API and the CLI require further
contract coverage and unchanged-consumer tests.
