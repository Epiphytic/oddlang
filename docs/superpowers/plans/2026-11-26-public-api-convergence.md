# Converge public API recovery in one seven-member workspace

- [x] Compile and qualify the public constant and typed error recovery commands in the same offline compiler. Run both unrelated fixtures and the library tests. Validate the design graph.
- [ ] Compose the qualified engine and native package inputs with the prototype constant package. Verify each member's manifest and source digest against its independently qualified parent. Preserve the existing workspace Cargo lock.
- [ ] Rebuild the workspace on both native ARM64 hosts with original Rust inputs absent. Test public observations against both original and generated libraries, including exact constant value, typed errors, native choice and preserved reader behavior.
- [ ] Rerun the public import audit against the combined Darwin build. Publish the input hashes, provenance, per-import outcomes and the remaining gaps. Obtain an exact Claude auto-mode review for each phase and merge the companion evidence.

The integration compiler combines public constant recovery at `c70bc9b` and typed error recovery at `85217d0`. The public constant and typed finite error fixtures passed with its compiled CLI. All 13 library tests passed with `cargo +1.96.1 test --target-dir /tmp/odd-buffer-compiler --locked --offline --lib` and no inherited `CARGO_TARGET_DIR`. The round-trip importer fixture inspects Cargo’s target location, so exporting that variable for the test process changes its temporary package behavior.
