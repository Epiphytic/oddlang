#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 5 ]]; then
    echo "usage: $0 UPSTREAM IMPORT PROJECTION EMPTY_OUTPUT MUTANT_PROJECTION" >&2
    exit 2
fi
upstream=$(cd "$1" && pwd)
import=$(cd "$2" && pwd)
projection=$(cd "$3" && pwd)
mkdir -p "$4"
out=$(cd "$4" && pwd)
mutant=$(cd "$5" && pwd)
if [[ -n $(ls -A "$out") ]]; then
    echo "framing evidence requires an empty output directory" >&2
    exit 1
fi
cd "$(dirname "$0")/../.."
root=$PWD
source examples/wasm/toolchain.lock
export CARGO_NET_OFFLINE=true RUSTUP_AUTO_INSTALL=0 PYTHONDONTWRITEBYTECODE=1
template="$root/examples/wasm/framing"
stage=$(mktemp -d "$out/.build.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
rustc +"$rust_version" -vV > "$out/rustc.txt"
grep -Fx "commit-hash: $rust_commit" "$out/rustc.txt" >/dev/null
printf '%s  %s\n' "$archive_sha256" "$cache/$archive_name.tar.gz" | shasum -a 256 -c -
tar -xzf "$cache/$archive_name.tar.gz" -C "$stage" "$archive_name/wasm-tools"
tools="$stage/$archive_name/wasm-tools"
"$tools" --version > "$out/wasm-tools.txt"

cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin frame-matrix -- verify-inputs "$import" "$projection" "$mutant"
cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin framing-observer -- collect "$upstream" "$out/original"
cargo +"$rust_version" build --locked --offline --manifest-path examples/wasm/Cargo.toml \
    --target-dir target/wasm-host --bin framing-host
cp target/wasm-host/debug/framing-host "$stage/host"
"$tools" component embed --dummy --world framing "$template/host.wit" -o "$stage/reference-core.wasm"
"$tools" component new "$stage/reference-core.wasm" -o "$stage/reference.wasm"
"$tools" component wit "$stage/reference.wasm" -o "$out/expected.wit"

build_pair() {
    local name=$1 projected=$2
    local dest="$out/$name" project="$stage/$name"
    mkdir -p "$dest" "$project"
    cp "$template"/Cargo.{toml,lock} "$template/batch.rs" "$project"
    cp -R "$template/core" "$template/consumer" "$template/guest" "$project"
    cp "$projected/frame.rs" "$project/core/src/frame.rs"
    mkdir -p "$project/guest/wit"
    cp "$projected/world.wit" "$project/guest/wit/world.wit"
    cargo +"$rust_version" build --locked --offline --manifest-path "$project/Cargo.toml" \
        --target-dir "$stage/target" --release -p framing-consumer
    cp "$stage/target/release/framing-consumer" "$dest/consumer"
    cargo +"$rust_version" build --locked --offline --manifest-path "$project/Cargo.toml" \
        --target-dir "$stage/target" --target "$ledger_guest_target" --profile "$guest_profile" -p framing-guest
    "$tools" component new "$stage/target/$ledger_guest_target/$guest_profile/framing_guest.wasm" \
        -o "$dest/component.wasm"
    "$tools" validate "$dest/component.wasm"
    "$tools" component targets --world framing "$projected/world.wit" "$dest/component.wasm"
    "$tools" component wit "$dest/component.wasm" -o "$dest/extracted.wit"
    cmp "$out/expected.wit" "$dest/extracted.wit"
}
build_pair regenerated "$projection"
build_pair mutant "$mutant"
cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin frame-matrix -- compare "$out" "$stage/host" "$import" "$projection" "$mutant"
