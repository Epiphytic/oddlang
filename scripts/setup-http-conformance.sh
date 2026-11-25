#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

require_version() {
    local tool=$1 expected=$2 actual
    shift 2
    if ! actual=$("$tool" "$@"); then
        printf 'Required tool: %s (%s)\n' "$tool" "$expected" >&2
        exit 1
    fi
    case "$actual" in
        "$expected"|"$expected "*) ;;
        *)
            printf '%s: expected %s, found %s\n' "$tool" "$expected" "$actual" >&2
            exit 1
            ;;
    esac
}

require_version rustc 'rustc 1.96.1' --version
require_version node v22.22.2 --version
require_version npm 10.9.7 --version
require_version go 'go version go1.26.3' version

tools="$root/target/http-tools"
schema="openapi-3.1-2022-10-07.json"
schema_sha=da01ba28852cac0de53893797cb8d1942bc3b05084f526dcc216717dec314ed0

mkdir -p "$tools"
cp scripts/http-tools/package.json scripts/http-tools/package-lock.json "$tools/"
npm --prefix "$tools" ci --ignore-scripts
"$tools/node_modules/.bin/playwright" install chromium
curl --fail --silent --show-error --location https://spec.openapis.org/oas/3.1/schema/2022-10-07 \
    --output "$tools/$schema"
(cd "$tools" && printf '%s  %s\n' "$schema_sha" "$schema" | shasum -a 256 --check)

cargo fetch --locked
cargo fetch --locked --manifest-path examples/contracts/monorepo/native/Cargo.toml
