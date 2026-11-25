#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
source scripts/http-tools/check-pins.sh
check_http_runtime

tools="$root/target/http-tools"
mkdir -p "$tools"
cp scripts/http-tools/package.json scripts/http-tools/package-lock.json "$tools/"
npm --prefix "$tools" ci --ignore-scripts
"$tools/node_modules/.bin/playwright" install chromium
check_http_tools "$tools"
curl --fail --silent --show-error --location https://spec.openapis.org/oas/3.1/schema/2022-10-07 \
    --output "$tools/$http_schema"
check_http_schema "$tools"

cargo fetch --locked
cargo fetch --locked --manifest-path examples/contracts/monorepo/native/Cargo.toml
cargo fetch --locked --manifest-path examples/http/provider/Cargo.toml
cargo fetch --locked --manifest-path examples/http/contract/shapes/v1/rust-runner/Cargo.toml

# Precache the legacy monorepo compiler and every locked hybrid dependency.
npm exec --yes --package=typescript@5.9.3 -- tsc --version
hybrid="$root/target/http-hybrid-deps"
mkdir -p "$hybrid"
cp examples/http/hybrid/package.json examples/http/hybrid/package-lock.json "$hybrid/"
npm --prefix "$hybrid" ci --ignore-scripts
check_hybrid_tools "$hybrid"
