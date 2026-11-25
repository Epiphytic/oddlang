#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

if [[ -z "${ODDLANG_TEST_DATABASE_URL:-}" ]]; then
    printf '%s\n' 'ODDLANG_TEST_DATABASE_URL is required for the pinned PostgreSQL ledger tests. See README.md.' >&2
    exit 1
fi
for required in target/http-tools/node_modules/.bin/tsc target/http-tools/openapi-3.1-2022-10-07.json; do
    if [[ ! -e "$required" ]]; then
        printf 'Missing %s. Run scripts/setup-http-conformance.sh first.\n' "$required" >&2
        exit 1
    fi
done

export CARGO_NET_OFFLINE=true
export GOTOOLCHAIN=local
export GOPROXY=off
export npm_config_offline=true
cargo test --locked --offline

bundle_root=contracts/cache/http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4
receipts=target/http-conformance
verify() {
    local version=$1 claim=$2 name=$3
    cargo run --quiet --locked --offline -- http verify-evidence "$bundle_root/$version" \
        "examples/http/contract/$version/workspace.lock.json" "$claim" "$receipts/$name.json"
}

for version in v1 v2; do
    verify "$version" examples/http/provider/claim.json "provider-$version"
    verify "$version" examples/http/clients/typescript/claim.json "typescript-$version"
done
verify v1 examples/http/clients/go/claim.json go-v1

shapes=examples/http/contract/shapes/v1
shape_bundle=contracts/cache/http/d550a36a22266cc54b0f99128f2851810bac7a782e5bdd519239f260a1f7392e/v1
for language in rust typescript go; do
    cargo run --quiet --locked --offline -- http verify-evidence "$shape_bundle" \
        "$shapes/workspace.lock.json" "$shapes/claim.json" "$receipts/shapes-$language.json"
done
printf 'Verified provider and client evidence under %s.\n' "$receipts"
