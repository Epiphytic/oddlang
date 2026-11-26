# Bounded local owner workflow verification

The new field-meter fixture first failed on the previous compiler with an unknown workflow field. On macOS ARM64 with Rust 1.97.1, it built a linked native CLI and compared ten cases against its independent Unix socket fixture. The positive cases checked exact probe, reserve and sample request frames, response transition precedence when two predicates match, retry, and explicit carry. The negative cases checked malformed capture, duplicate reply members, missing CLI input, oversized reply, deadline and exchange budget. The compiler rejected a capture unavailable on an incoming path, an undeclared carry and a minimum request larger than its frame limit.

Both tests ran through their ordinary generated CLI binaries. The earlier stateless field-meter fixture passed without changes. Concatenating the shared exchange template and stateless request wrapper reproduces the earlier transport template byte for byte. The extracted strict JSON decoder preserves the prior HTTP parsing source. All 16 release library tests and all three HTTP provider tests passed. Distinct-UID rejection lacks a native witness on this host. The workflow inherits the existing effective-UID guard and requires the pinned JSON backend without arbitrary-precision extension.

```sh
cargo +1.97.1 build --locked --offline --release --bin oddlang
python3 -B verification/local-owner-workflow/check.py target/release/oddlang
python3 -B verification/local-command-client/check.py target/release/oddlang
cargo +1.97.1 test --locked --offline --release --lib
```

Claude Opus 5.5 approved the scoped primitive in Claude Code auto mode. The review found that captures could otherwise survive a path that failed to refresh them. Incoming-state analysis and explicit transfer now constrain their lifetime. Request encoding, build-time size checks and fixture coverage were expanded after review.

The companion's [pinned upstream local-owner witness](https://github.com/Epiphytic/marmish-oddlang/blob/2c986260f6cbebf489b3330899ab1067fe03b7b7/verification/whole/basic-command-interop/upstream-local-owner-aarch64-apple-darwin.json) separately measures status, room preparation and submit. The workflow's fixture establishes the reusable mechanics. Marmish's local application policy and the actual generated client and owner need separate runtime evidence.
