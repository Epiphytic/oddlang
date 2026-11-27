# Public data compiler composition

The compiler at `900cfa517d0136fa80c2b5e92ccf9b1a3b2a71ab` combines pointer-width record recovery, checked Rust and TypeScript choices, clone-only serializable choices and the Serde string-choice importer. The two original native composition receipts and their paired inherited receipts record the historical runs on Darwin and Linux ARM64. Each host used one compiler executable throughout both stages.

Run the Rust verifier from the current checkout. Set `RUNTIME_CHECKOUT` to a clean checkout at the commit above and `PINNED_TARGET` to an external Cargo target directory. The default command builds the compiler with Rust 1.96.1. The qualification programs use Rust 1.97.1 and locked offline dependencies. The optional Node consumer requires Node and cached TypeScript 5.9.3. Without `--with-node`, the compiler still exercises the accepted and rejected TypeScript choice profiles.

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml \
  --bin public-data-composition -- verify "$RUNTIME_CHECKOUT" "$PINNED_TARGET" \
  /tmp/public-data-composition.json --with-node
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml \
  --bin public-data-composition -- inherited "$RUNTIME_CHECKOUT" "$PINNED_TARGET/debug/oddlang" \
  /tmp/public-data-composition.json /tmp/inherited.json
```

To reproduce a frozen executable run, add `--binary "$ARCHIVED_ODDLANG"` to `verify` and pass that same executable to `inherited`. This mode checks the executable against the recorded SHA-256 for the current host before running fixtures. It still compiles and runs the 13 predecessor library tests from the pinned source. The inherited stage can also take the original host composition receipt as its third positional argument.

Each current fixture receives the same compiler. Its report pairs the historical fixture path, digest and captured output with the current Rust program, its binary and runtime-input digests, and its own output. Every historical fixture is checked against the pinned checkout. The verifier checks the compiler digest and checkout cleanliness again before writing a receipt. The inherited stage checks the host, executable, predecessor inventory and named tests against its parent receipt, then replays the eight predecessor fixtures and general plain-choice fixture.

The Darwin replay used the archived executable for six composition fixtures and nine inherited fixtures. Its paired receipts are [composition](../provider-tooling/public-data-native-composition.json) and [inherited](../provider-tooling/public-data-native-inherited.json). The companion project maintains the separately pinned application workspace and downstream import evidence.
