#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 ]] || { echo "usage: $0 BUNDLES OUT" >&2; exit 2; }
bundles=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
cd "$(dirname "$0")/.."
source examples/wasm/toolchain.lock
export CARGO_NET_OFFLINE=true RUSTUP_AUTO_INSTALL=0
rustc +"$rust_version" -Vv | grep -Fx "commit-hash: $rust_commit"
printf '%s  %s\n' "$archive_sha256" "$cache/$archive_name.tar.gz" | shasum -a 256 -c -
stage=$(mktemp -d "$out/.build.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
tar -xzf "$cache/$archive_name.tar.gz" -C "$stage" "$archive_name/wasm-tools"
tools="$stage/$archive_name/wasm-tools"

# Keep the exact generated Rust and WIT inputs available for clean-directory comparisons.
src="$out/source"
mkdir -p "$src/ledger/wit"
template=examples/wasm/ledger-guests
cp "$template"/Cargo.{toml,lock} "$src/ledger/"
cp -R "$template/src" "$src/ledger/"
for package in ledger ledger-test; do
    mkdir -p "$src/ledger/wit/$package"
    cp -R "$bundles/$package/wit/." "$src/ledger/wit/$package/"
done
cargo +"$rust_version" build --locked --offline --bin oddlang --target-dir target/wasm-publisher
target/wasm-publisher/debug/oddlang build examples/ledger/v1 "$stage/native"
cp "$stage/native/account.rs" "$src/ledger/src/account.rs"

build() {
    local project=$1 name=$2 features=$3 wit=$4 world=$5
    cargo +"$rust_version" build --locked --offline --manifest-path "$project/Cargo.toml" \
        --target-dir "$stage/target" --target "$ledger_guest_target" --profile "$guest_profile" \
        --features "$features"
    local artifact="$out/$name.wasm"
    "$tools" component new "$stage/target/$ledger_guest_target/$guest_profile/ledger_guest.wasm" -o "$artifact"
    "$tools" validate "$artifact"
    "$tools" component targets --world "$world" "$wit" "$artifact"
    "$tools" component wit "$artifact" -o "$out/$name.wit"
    # Compare the full canonicalized import/export surface, including nominal types.
    "$tools" component embed --dummy --world "$world" "$wit" -o "$stage/reference-core.wasm"
    "$tools" component new "$stage/reference-core.wasm" -o "$stage/reference.wasm"
    "$tools" component wit "$stage/reference.wasm" -o "$stage/reference.wit"
    diff -u "$stage/reference.wit" "$out/$name.wit"
}

for implementation in manual generated; do
    features=""
    [[ $implementation == manual ]] || features=generated
    build "$src/ledger" "$implementation-ledger" "$features" "$bundles/ledger/wit" ledger
    build "$src/ledger" "$implementation-ledger-test" "${features:+$features,}instrumented" \
        "$bundles/ledger-test/wit" ledger-test
done
for mutation in trap double-import max-exclusive probe; do
    build "$src/ledger" "$mutation" "$mutation" "$bundles/ledger/wit" ledger
done
for name in echo echo-rename echo-tags echo-import; do
    [[ -d "$bundles/$name/wit" ]] || continue
    project="$src/$name"
    mkdir -p "$project/src"
    cp "$template"/Cargo.{toml,lock} "$project/"
    cp "$template/matrix/echo.rs" "$project/src/lib.rs"
    cp -R "$bundles/$name/wit" "$project/wit"
    case "$name" in
        echo-rename) features=rename ;;
        echo-import) features=seed ;;
        *) features="" ;;
    esac
    build "$project" "$name" "$features" "$bundles/$name/wit" echo-world
done
