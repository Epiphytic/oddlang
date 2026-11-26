#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
revision=49fc705408a230bd0ad896248d08dbdd2faad05a
upstream=${ODDLANG_MARMISH_SOURCE:-$PWD/target/marmish/upstream}
if [[ ! -d "$upstream" ]]; then
    mkdir -p "$(dirname "$upstream")"
    git clone --no-checkout https://github.com/Epiphytic/marmish.git "$upstream"
    git -C "$upstream" checkout --detach "$revision"
fi
if [[ $(git -C "$upstream" rev-parse HEAD) != "$revision" ]]; then
    echo "Expected Marmish revision $revision in $upstream" >&2
    exit 1
fi
(
    cd "$upstream"
    rustc -Vv
    cargo fetch --locked
    uv sync --locked --python 3.12 --project tools/code-ontology
)
cargo +1.97.1 fetch --locked --manifest-path examples/rust-import/framing-independent/Cargo.toml
cargo +1.97.1 fetch --locked --manifest-path examples/wasm/framing/Cargo.toml
cargo +1.97.1 fetch --locked --manifest-path examples/wasm/framing/Cargo.toml --target wasm32-unknown-unknown
