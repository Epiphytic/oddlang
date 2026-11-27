# Replaying provider compatibility on macOS ARM64

Build the validation binaries with `cargo build --locked --offline --manifest-path verification/provider-validation/Cargo.toml`. The record-composition tool compares standalone record recovery, projection and native package composition across two Oddlang executables. Its JSON receipt includes both executable hashes and hashes for the eleven byte-identical artifacts.

```sh
verification/provider-validation/target/debug/record-composition "$OLD_CLI" "$NEW_CLI"
```

The public-record qualification tools compile independent consumers against source or reconstructed crates using Rust 1.97.1 and the offline Cargo cache. Run each tool against the candidate compiler.

```sh
for check in record-verify record-read-policy record-recover-serde record-source-api; do
  "verification/provider-validation/target/debug/$check" "$NEW_CLI" || exit 1
done
```

The fixtures under `fixtures/public-records` retain the independently authored caller source and input data. The four tools exercise module aliases and source mutation, open and closed Serde readers, Rustdoc recovery with positional field order, and source-level field requirements.

The workspace replay takes the 111 files listed in [marmish-inputs.sha256](marmish-inputs.sha256). Each path is relative to `oddlang/portable` in [the pinned companion release](https://github.com/Epiphytic/marmish-oddlang/tree/058e5d3f473380ced323db756a96d37a6a37476a/oddlang/portable). To prepare a new directory from a companion checkout, use an absolute path for `COMPANION` and an empty directory for `INPUTS`.

```sh
set -e
while read -r digest path; do
  mkdir -p "$INPUTS/$(dirname "$path")"
  git -C "$COMPANION" show "058e5d3:oddlang/portable/$path" > "$INPUTS/$path"
done < verification/provider-validation/marmish-inputs.sha256
"$OLD_CLI" build native-workspace rust "$INPUTS/workspace-command-policy.json" "$BASELINE_TREE"
```

Build the Rust command-policy validator from [companion revision 9731a36](https://github.com/Epiphytic/marmish-oddlang/tree/9731a3625c4eff8400184dc5cbd1875b761e2e90/verification/whole/command-policy). Supply a local original Marmish Git checkout containing commit `af54339d250290490d62d4475a77539b7e739071`, the pinned Rust 1.97.1 toolchain and a populated locked offline Cargo cache. Run from the Oddlang checkout.

```sh
cargo build --locked --offline --manifest-path "$COMPANION/verification/whole/command-policy/Cargo.toml"
verification/provider-validation/target/debug/provider-validation \
  "$NEW_CLI" "$INPUTS" "$BASELINE_TREE" \
  "$COMPANION/verification/whole/command-policy/target/debug/command-policy-validation" \
  "$ORIGINAL" "$CARGO_TARGET_CACHE" "$NEW_RECEIPT"
```

Set `ODDLANG_VALIDATION_SCRATCH` to an existing directory on a volume with several GiB free to relocate the reconstruction and its disposable Cargo builds. The validator creates a private subdirectory there and removes it when the run ends. The policy observers inherit the caller's usual temporary-directory settings.

The validator checks every input against its pinned digest, renders into a new temporary directory and compares directory entries, permissions and file contents with the baseline. It then builds the original and generated observers, replays the independent policy and file cases, checks the historical observation and input digests, and writes the receipt. The observer processes run on local files and in-memory inputs.

Run the native descriptor-limit and startup checks with `process-limit` and `process-startup`, and the finite-error recovery and projection cases with `finite-error-domain`. Each binary takes the compiler executable as its sole argument.

The `wasm-reports` binary reads the first and second guest builds, JavaScript host receipts and Wasmtime observations under a verification root. Its optional second argument names an existing output directory, so an archived run can be rechecked without writing into it.
