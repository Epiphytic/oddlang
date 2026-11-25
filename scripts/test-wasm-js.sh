#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
artifacts=$(cd "$1" && pwd)
reports=$2
js="$PWD/target/wasm-js"
cmp examples/wasm/javascript/package-lock.json "$js/package-lock.json"
cmp examples/wasm/javascript/package.json "$js/package.json"
[[ $(node --version) == v22.22.2 ]]
[[ $("$js/node_modules/.bin/jco" --version) == 1.42.0 ]]
[[ $("$js/node_modules/.bin/tsc" --version) == 'Version 5.9.3' ]]
cp examples/wasm/javascript/run.mjs "$js/run.mjs"
mkdir -p "$reports"
for impl in manual generated; do
    for world in ledger ledger-test; do
        name="$impl-$world"
        component="$artifacts/guests/$name.wasm"
        binding="$reports/$name"
        "$js/node_modules/.bin/jco" transpile "$component" --instantiation async \
            --no-wasi-shim --name ledger --out-dir "$binding"
        printf '%s\n' '{"type":"module"}' > "$binding/package.json"
        cp examples/wasm/javascript/host.ts "$binding/"
        "$js/node_modules/.bin/tsc" --strict --target es2022 --module esnext \
            --moduleResolution bundler "$binding/host.ts"
        node "$js/run.mjs" "$artifacts/$world" "$component" "$binding" "$reports/$name"
    done
done
