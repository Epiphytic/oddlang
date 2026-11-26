# Converge public API recovery in one seven-member workspace

- [x] Compile and qualify the public constant and typed error recovery commands in the same offline compiler. Run both unrelated fixtures and the library tests. Validate the design graph.
- [x] Compose the qualified engine and native package inputs with the prototype constant package. Verify each member's manifest and source digest against its independently qualified parent. Preserve the existing workspace Cargo lock.
- [x] Rebuild the workspace on both native ARM64 hosts with original Rust inputs absent. Test public observations against both original and generated libraries, including exact constant value, typed errors, native choice and preserved reader behavior.
- [x] Rerun the public import audit against the combined Darwin build. Publish the input hashes, provenance, per-import outcomes and the remaining gaps. Obtain an exact Claude auto-mode review for each phase and merge the companion evidence.

The integration compiler combines public constant recovery at `c70bc9b` and typed error recovery at `bbdc69e`. The public constant and typed finite error fixtures passed with its compiled CLI. All 13 library tests passed with `cargo +1.96.1 test --target-dir /tmp/odd-buffer-compiler --locked --offline --lib` and no inherited `CARGO_TARGET_DIR`. The round-trip importer fixture inspects Cargo’s target location, so exporting that variable for the test process changes its temporary package behavior.

The [combined workspace qualification](https://github.com/Epiphytic/marmish-oddlang/pull/89) merged at `4f39fe3da13635d66b5a09fbe55e52fe27d17962`. The cold builds on Linux and Darwin ARM64 agree on the generated workspace lock and public engine, identity and constant observations. The Darwin caller audit increased importability from 65 to 70 of the 140 original names. The named suite and mixed-peer interoperability require separate evidence.
