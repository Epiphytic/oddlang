# Public data compiler composition

The runtime at `900cfa517d0136fa80c2b5e92ccf9b1a3b2a71ab` combines
pointer-width record recovery, checked fieldless choices in Rust and
TypeScript, clone-only serializable choices, and the source importer for
Serde string choices. Each fixture receives the same compiler executable
within a host. The verifier checks its digest before and after the run.

Use a clean checkout of that runtime and an external Cargo target. The script
builds the compiler with Rust 1.96.1. The Rust callers use 1.97.1 and cached
dependencies. The optional Node check needs Node and cached TypeScript
5.9.3. Both native ARM64 receipts include the Rust checks and a CLI check of
which choices the TypeScript backend accepts. Darwin also runs the actual
TypeScript consumer. Each host must pass the same named library tests as the
previous combined compiler.

```sh
python3 -B verification/public-data-composition/verify.py \
    "$RUNTIME_CHECKOUT" "$CARGO_TARGET_DIR" /tmp/public-data-composition.json --with-node
```

The result records tool versions, fixture and compiler digests, and named
library tests. The companion project maintains the separately pinned
application workspace and downstream import evidence.
