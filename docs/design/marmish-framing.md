# Marmish module recovery

`oddlang import rust` recognizes the closed framing profile in the pinned Rust source. `oddlang build module` reads its graph and import lock, and produces native Rust, the WIT contract and a Rust projection manifest. `oddlang import replay` re-runs source recovery. `oddlang import verify-build` reprojects all three artifacts and compares the bytes. The separate assembly command reconstructs a workspace using the pinned projection and retained inputs.

```sh
scripts/setup-marmish.sh
scripts/setup-wasm.sh
scripts/qualify-marmish.sh frame
cargo test --locked --offline --test framing_import --test framing_projection --test workspace_assembly
```

The [recognizer specification](../language/rust-framing-recognizers.md) gives the accepted Rust syntax, crate dependencies and diagnostic boundary. The [assembly format](../language/workspace-assembly.md) defines each file producer and digest. The public-interface test uses a second Rust crate with a seven-byte limit, version seven and different guard order. It deletes the original crate directory before emitting the native module, compiles a new consumer and rejects a stale WIT artifact.

The component exports encode and decode through a pure WIT interface and contains no host imports. The native and Wasm projections reuse the documented Rust crate versions for JSON, derives and base64. The expected bytes and named errors come from the frozen [observation contract](../../examples/marmish/observations.json). They include overlapping invalid conditions and both sides of the resource boundaries.

The [remaining module register](../../examples/marmish/gaps.json) identifies 97 retained files with 5,832 lexical declaration sites. Seventeen sites in the framing module are in the recovered source slice. The denominator for compiled runtime declarations remains unknown. Recreate the register with `python3 scripts/marmish/gaps.py INVENTORY.json.gz gaps.json`. Each module links to a named work-graph task and a proposed fixture with positive and rejection criteria.

The [framing archive](../../examples/marmish/evidence/framing.tar.gz) contains both semantic graphs, both source projections, instantiated component bytes, and the native and Wasm observations for the positive and negative cases. Two independent builds emitted the same regenerated component SHA-256, `05a793337c0c7154faa7a01ff66ec2837688a231876eb3843af2fd2cb68ccc05`. Both guard-order mutants changed precisely the version-before-size outcome.

The assembly smoke test reconstructed the actual 1,641-path upstream tree, substituting only the projected framing module. All seven unchanged upstream `marmish-core` integration tests passed when compiled against this tree.

The reviewed language profile recognizes one closed protocol family. The second fixture exercises independent values and allowed error ordering within this family. New library calls, control structures or wire formats require a new specified recognizer.

The qualifier builds a complete project manifest, reads retained Git blobs from the pinned tree and materializes them under their content digests. A separately checked framing release anchors the import and projection. The assembler validates the pinned import, regenerates the Rust module in scratch space, and compares it with the supplied bytes. The upstream core contract tests run inside the assembled workspace. A changed retained blob must fail before publication.

The corrected gate pinned the import lock at `e39cf8da02d75442a98c1a338f694931ce5b7b2b5cb70c2759c50f1efea8bdcc` and the complete project manifest at `d4e3466831b0506ce0418db35afdbf93a65b544a09c475457a239431f54faf1d`. It measured 90 assembled compiler inputs, including the generated module and retained native build script. A modified retained build script received PermissionDenied when it tried to read an external canary. The [build boundary record](marmish-build-boundary.md) names the permitted reads and the limits of dep-info. The complete [replay archive](../../examples/marmish/evidence/framing.tar.gz) includes the actual assembly manifest and transcripts.

Claude CLI approved the corrected implementation at `36c6fb3` after reproducing the framing gate, the new assembly and provenance rejection controls, and compiler closure on native Apple Silicon. The closure run checks the core and native libraries under default features. Other crates and features have separate runtime evidence.
