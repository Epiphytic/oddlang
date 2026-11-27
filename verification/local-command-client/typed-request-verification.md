# Typed local command request verification

The field-meter test fixture first failed on Oddlang `8a20ea283d0d43da42f054a7c7774eb9e6a20da7` with `unknown field argument, expected key or value`. The fixture binds unrelated device read arguments to an existing generated executable, and its independent Unix listener checks the declared JSON bytes, frame size and half-close before returning a reply.

On macOS ARM64 with Rust `1.97.1`, the modified compiler built the package and passed the fixture. The observed cases include a 128-byte request and a rejected 129-byte request, the maximum unsigned 64-bit integer, escaped quote and control characters, preserved raw UTF-8, repeated argument order, explicit null, missing values, invalid field identities and three native CLI binding type mismatches. CLI defaults for scalar and repeated values were ignored as specified. A request whose shortest encoding exactly meets its configured limit was accepted at build time. A separate check verifies the emitted request-obligation catalog and projection lock digest. The fixture SHA-256 is `fdf13c950ad41106a59167e85d27e4b96bafb43a34a3db7d9ef801b5dc7ff0b6`.

The generated source and locks for the original literal field-meter graph and the pinned Marmish status graph matched the predecessor projection byte for byte. The generated source SHA-256 digests were `f27077cb299b6d289f00a35c179d43edd6cdbd3d1034028678f1934eb5bf9fc8` and `145e9a8e1882fc4fa94113965c37629839a18613f426b4148c929c8756dcd6d5`. All 16 release library tests passed after applying the change to main at `9148a63a711c78ea945c688d395c778ec32d2fe3`. Both Turtle files parsed and the ready-work query executed with Oxigraph.

```sh
cargo +1.97.1 build --release --locked --offline --bin oddlang
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin field-meter-projection -- target/release/oddlang
python3 -B verification/local-command-client/check.py target/release/oddlang
cargo +1.97.1 test --release --locked --offline --lib
```

The standalone Rust command checks the request and reply catalogs and eleven invalid graphs. The full field-meter fixture also invokes it and exercises the generated client through a local Unix socket.

Claude Opus 5.5 approved the final code in Claude Code auto mode. Earlier review findings identified missing argument and field obligations, escape and number encoding details, edge-size coverage, and implicit CLI defaults overriding the request catalog. The final code binds the catalog to stable declaration IRIs, records exact encoding rules, and uses only values explicitly supplied on the command line. The reviewer inspected code and the fixture. The runtime observations above came from separate verification runs.

The field-meter test qualifies the reusable local request projection. The pinned [Marmish owner handoff](https://github.com/Epiphytic/marmish-oddlang/blob/597dda6f9331cbb3521e3f4c679ef1b64d1939dc/verification/whole/basic-command-interop/LOCAL-OWNER.md) separately records the original client. A generated Marmish client still needs profile and target admission, readiness and preparation sequencing, and submit and wait result handling.
