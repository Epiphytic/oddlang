#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../../.." && pwd)
prefix="${ODDLANG_PROOF_TOOLS:-$root/target/proof-tools}"
export PATH="$prefix/bin:$PATH"
if ! cargo install --list --root "$prefix" | grep -Fqx 'kani-verifier v0.68.0:'; then
    cargo install --locked --version '=0.68.0' --root "$prefix" kani-verifier
fi
if [[ ! -x "${KANI_HOME:-$HOME/.kani/kani-0.68.0}/bin/cbmc" ]]; then
    cargo kani setup
fi
cargo kani --version
