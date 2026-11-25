#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source examples/wasm/toolchain.lock

mkdir -p "$cache"
stage=$(mktemp -d "$cache/setup.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
curl -fsSL "https://static.rust-lang.org/dist/channel-rust-$rust_version.toml" -o "$stage/rust.toml"
printf '%s  %s\n' "$rust_manifest_sha256" "$stage/rust.toml" | shasum -a 256 -c -
rustup toolchain install "$rust_version" --profile minimal --target "$guest_target"
curl -fsSL "https://github.com/bytecodealliance/wasm-tools/releases/download/v$wasm_tools_version/$archive_name.tar.gz" \
    -o "$stage/$archive_name.tar.gz"
printf '%s  %s\n' "$archive_sha256" "$stage/$archive_name.tar.gz" | shasum -a 256 -c -
mv "$stage/$archive_name.tar.gz" "$cache/"
cargo +"$rust_version" fetch --locked --manifest-path examples/wasm/Cargo.toml
cargo fetch --locked --manifest-path Cargo.toml
cargo +"$rust_version" fetch --locked
