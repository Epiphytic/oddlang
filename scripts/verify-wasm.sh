#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source examples/wasm/toolchain.lock
export CARGO_NET_OFFLINE=true
export RUSTUP_AUTO_INSTALL=0
export npm_config_offline=true

prepare() {
    local out=$1 release_store ledger_key
    mkdir -p "$out"
    if [[ -n "$(ls -A "$out")" ]]; then
        echo "Component verification needs an empty output directory: $out" >&2
        return 1
    fi
    out=$(cd "$out" && pwd)
    printf '%s  %s\n' "$archive_sha256" "$cache/$archive_name.tar.gz" | shasum -a 256 -c -
    tar -xOzf "$cache/$archive_name.tar.gz" "$archive_name/wasm-tools" > "$out/wasm-tools"
    chmod +x "$out/wasm-tools"
    cargo build --locked --offline --bin oddlang --bin publish-wasm-matrix

    ledger_key=$(printf %s 'urn:oddlang:ledger:wasm:package' | shasum -a 256 | cut -d ' ' -f 1)
    release_store="$out/cache"
    cargo run --quiet --locked --offline --bin oddlang -- wasm publish contracts/wasm/ledger contracts/wasm/empty.lock.json "$release_store" "$out/ledger"
    mkdir -p "$release_store/wasm/$ledger_key"
    cp -R "$out/ledger" "$release_store/wasm/$ledger_key/1.0.0"
    cargo run --quiet --locked --offline --bin oddlang -- wasm publish contracts/wasm/ledger-test contracts/wasm/ledger-test/dependencies.lock.json "$release_store" "$out/ledger-test"
    cargo run --quiet --locked --offline --bin oddlang -- wasm publish contracts/wasm/echo contracts/wasm/empty.lock.json "$release_store" "$out/echo"
    cargo run --quiet --locked --offline --bin publish-wasm-matrix -- "$out"
    cargo +"$rust_version" run --locked --offline --manifest-path verification/provider-validation/Cargo.toml \
        --bin wasm-guests -- "$out" "$out/guests"
    cargo +"$rust_version" build --locked --offline --manifest-path examples/wasm/Cargo.toml \
        --target-dir target/wasm-host -p runtime-host
    cp target/wasm-host/debug/runtime-host "$out/runtime-host"
}

if [[ $# == 2 && $1 == --prepare ]]; then
    prepare "$2"
    exit
fi
if [[ $# != 0 ]]; then
    echo 'usage: scripts/verify-wasm.sh [--prepare EMPTY_DIRECTORY]' >&2
    exit 1
fi

root=$(mktemp -d "$cache/verification.XXXXXXXX")
for attempt in first second; do prepare "$root/$attempt"; done
for bundle in ledger ledger-test ledger-probe echo echo-label echo-rename echo-tags echo-import; do
    diff -qr "$root/first/$bundle" "$root/second/$bundle"
done
export ODDLANG_WASM_ARTIFACTS="$root/first"
export ODDLANG_WASM_HOST="$root/first/runtime-host"
export WASM_TOOLS="$root/first/wasm-tools"
cargo test --locked --offline --test wasm_runtime

cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin wasm-js-hosts -- "$root/first" "$root/hosts"
cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin wasm-reports -- "$root"
echo "Verified Wasmtime, Node and Chromium evidence in $root"
