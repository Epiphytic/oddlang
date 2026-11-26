# Flat sorted wire frames

Verified on macOS ARM64 using Rust 1.97.1 and the locked offline radio fixture. The previous compiler rejected the new layout declaration. The updated compiler generated and compiled both nested and flat projections. The fixture passed with warnings denied.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 verification/canonical-wire/verify.py target/debug/oddlang
```

The flat fixture checks the exact tag, payload and envelope order, recursive sorting, malformed and duplicate keys, an unsupported layout, a promoted key collision, the exact size limit and one byte over. Its case digest assertion pins the independent digest representation. Peer execution is tracked separately in the basic-command interoperability plan.
