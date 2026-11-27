#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 5 ]]; then
    echo "usage: $0 ORIGINAL_RS GENERATED_RS ORIGINAL_SHA256 GENERATED_SHA256 OUTPUT_DIR" >&2
    exit 2
fi
pkg=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$pkg/../.." && pwd)
export PATH="${ODDLANG_PROOF_TOOLS:-$root/target/proof-tools}/bin:$PATH"
export CARGO_NET_OFFLINE=true
export RUSTFLAGS='-C overflow-checks=yes'
exec cargo run --locked --offline --manifest-path "$root/verification/provider-validation/Cargo.toml" --bin translation-proof -- "$@"
