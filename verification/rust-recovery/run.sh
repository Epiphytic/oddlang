#!/usr/bin/env bash
# Compatibility entry point for executable-adapter profiles using the original path.
set -eu
suite=$(cd "$(dirname "$0")/../provider-validation" && pwd)
unset RUSTC RUSTFLAGS RUSTDOCFLAGS RUSTC_BOOTSTRAP RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset CARGO_ENCODED_RUSTFLAGS CARGO_ENCODED_RUSTDOCFLAGS CARGO_BUILD_TARGET
unset CARGO_BUILD_RUSTFLAGS CARGO_BUILD_RUSTDOCFLAGS CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER
while IFS= read -r key; do
    case "$key" in CARGO_PROFILE_*|CARGO_TARGET_*_RUSTFLAGS|CARGO_TARGET_*_RUSTDOCFLAGS) unset "$key";; esac
done < <(compgen -e)
exec cargo +1.97.1 run --quiet --locked --offline --manifest-path "$suite/Cargo.toml" --bin rust-capture -- "$@"
