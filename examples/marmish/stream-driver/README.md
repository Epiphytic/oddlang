# Independent Marmish stream oracle

The [frozen vectors](../stream-vectors.json) exercise the public API of
`marmish-stream` at upstream commit `49fc705408a230bd0ad896248d08dbdd2faad05a`.
The Rust driver observes values and errors through a linked library. The Rust
runner compares those observations against expectations derived before running
the original. Each fresh Cargo target supplies the artifact paths and digests
embedded in the executable binding alongside the pinned workspace identity.
The runner verifies those inputs and checks the executable's binding and
SHA-256 when it runs.

## Wire expectations

- Copy records start with a one-byte tag and a big-endian, four-byte payload
  length. Data uses tag 5, an eight-byte big-endian offset and raw payload.
  Checkpoint uses tag 6, an eight-byte offset and a 32-byte SHA-256. Progress
  uses tag 10, a one-byte stage and two big-endian eight-byte counts. Error
  uses tag 11 and two bytes for the error code and its owner. The corpus pins
  exact bytes for these records, includes a data payload of 8,192 bytes and
  checks the order of length, tag and offset errors. Objects with `prefix`,
  `byte` and `repeat` represent a compact prefix and repeated byte in hex.
- The stream JSON envelope uses compact JSON in protocol, version, binding,
  sequence, message order. Binding identities are fixed-length lowercase
  hex. The request digest is the SHA-256 of the complete canonical request
  bytes. The hello request and gap response are pinned byte for byte,
  including their direction, operation, and field order.
- The frozen codec expectations were derived with Python's SHA-256, base64 and zlib.
  The Rust runner checks the separately observed encoder using system zlib. The case requires a
  complete zlib stream with no leftover bytes and an exact decoded payload.
  This keeps the check independent of equivalent DEFLATE encoder choices.
- Fragmentation tests track consumed bytes, frame delivery and error
  persistence. Window tests track production, acknowledgement, queued bytes,
  credit and EOF before and after both successful and rejected operations.

## Freeze the upstream observation

Use the Rust 1.97.1 toolchain. The offline Cargo cache
must already contain the workspace lock's inputs. Use a fresh build target and
the stable upstream receipt from `scripts/marmish/stream_baseline.py`.

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml \
  --bin stream-driver -- --workspace /path/to/original \
  --target-dir /tmp/original-stream-oracle --role original \
  --receipt /tmp/original-stream-oracle.json --baseline /path/to/baseline/receipt.json
```

For the candidate and reverse replay use
`scripts/marmish/qualify_stream_workspace.py`. The qualification runner
supplies the reviewed commit, graph lock pin, archive and source isolation
profile. Both directions compare all peer observations against the frozen
vectors before replaying peer-emitted bytes.

The frozen upstream run matched all 50 observations. The driver and vector
SHA-256 digests are
`6b5caa2a7d19af3960248ae86e7f175d4d142939efa7074c65ea6126eced8b1e` and
`8bf576e1223d9cd916be67595b94ae4064aa7f3c87365a977bc5dd303273ec52`.
The interoperability comparison replays nine wire records in each direction.
A build-valid mutation that replaces the copy Data encoder tag 5 with 12
changes both exact-byte Data observations.

The Rust runner reproduced the complete historical runner receipt under equivalent nonincremental build inputs. Local original and candidate builds matched the archived observation rows in both directions. [Rust replay evidence](../../../verification/provider-tooling/stream-driver-rust-replay.json)
