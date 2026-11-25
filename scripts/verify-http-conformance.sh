#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
source scripts/http-tools/check-pins.sh

if [[ -z "${ODDLANG_TEST_DATABASE_URL:-}" ]]; then
    printf '%s\n' 'ODDLANG_TEST_DATABASE_URL is required for the pinned PostgreSQL ledger tests. See README.md.' >&2
    exit 1
fi
export CARGO_NET_OFFLINE=true
export GOTOOLCHAIN=local
export GOPROXY=off
export npm_config_offline=true

check_http_runtime
for required in target/http-tools/node_modules target/http-hybrid-deps/package-lock.json; do
    if [[ ! -e "$required" ]]; then
        printf 'Missing %s. Run scripts/setup-http-conformance.sh first.\n' "$required" >&2
        exit 1
    fi
done
check_http_tools target/http-tools
check_http_schema target/http-tools
hybrid=target/http-hybrid-deps
for file in package.json package-lock.json; do
    if ! cmp -s "examples/http/hybrid/$file" "$hybrid/$file"; then
        printf 'Hybrid %s is missing or differs from the setup pin. Run setup again.\n' "$file" >&2
        exit 1
    fi
done
if ! npm --prefix "$hybrid" ci --offline --ignore-scripts --no-audit; then
    printf '%s\n' 'Missing hybrid npm packages for the checked-in package-lock.json. Run setup again.' >&2
    exit 1
fi
check_hybrid_tools "$hybrid"

scratch=$(mktemp -d target/http-verify.XXXXXX)
trap 'rm -f "$scratch/start" "$scratch/go.lock.json"; rmdir "$scratch"' EXIT
touch "$scratch/start"
# Keep the HTTP and ledger gate independently runnable with its own setup.
cargo test --locked --offline --lib --bins --examples \
    --test adoption --test contract_release --test database --test evolution \
    --test http_bundle --test http_consumers --test http_evolution --test http_hybrid \
    --test http_model --test http_provider --test http_shapes --test impact --test model \
    --test model_targets --test monorepo --test oracle --test transfer_targets
cargo test --locked --offline --doc

bundle_root=contracts/cache/http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4
receipts=target/http-conformance
verify() {
    local bundle=$1 pin=$2 claim=$3 name=$4
    local evidence="$receipts/$name.json"
    if [[ ! "$evidence" -nt "$scratch/start" ]]; then
        printf 'Missing fresh evidence: %s\n' "$evidence" >&2
        exit 1
    fi
    cargo run --quiet --locked --offline -- http verify-evidence "$bundle" "$pin" "$claim" "$evidence"
}

for version in v1 v2; do
    pin="examples/http/contract/$version/workspace.lock.json"
    verify "$bundle_root/$version" "$pin" examples/http/provider/claim.json "provider-$version"
    verify "$bundle_root/$version" "$pin" examples/http/clients/typescript/claim.json "typescript-$version"
done
go_target=examples/http/clients/go
cargo run --quiet --locked --offline -- http verify-target-lock "$go_target" \
    "$go_target/target.lock.json" "$bundle_root/v1"
node -e 'const fs = require("node:fs"), [source, destination] = process.argv.slice(1);
    const target = JSON.parse(fs.readFileSync(source));
    fs.writeFileSync(destination, JSON.stringify({pins: [target.contract]}));' \
    "$go_target/target.lock.json" "$scratch/go.lock.json"
verify "$bundle_root/v1" "$scratch/go.lock.json" "$go_target/claim.json" go-v1

shapes=examples/http/contract/shapes/v1
shape_bundle=contracts/cache/http/d550a36a22266cc54b0f99128f2851810bac7a782e5bdd519239f260a1f7392e/v1
for language in rust typescript go; do
    verify "$shape_bundle" "$shapes/workspace.lock.json" "$shapes/claim.json" "shapes-$language"
done
printf 'Verified provider, client and codec evidence under %s.\n' "$receipts"
