#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source examples/wasm/toolchain.lock
export CARGO_NET_OFFLINE=true
export RUSTUP_AUTO_INSTALL=0
export npm_config_offline=true

prepare() {
    local out=$1 compiler release_store ledger_key
    mkdir -p "$out"
    if [[ -n "$(ls -A "$out")" ]]; then
        echo "Component verification needs an empty output directory: $out" >&2
        return 1
    fi
    out=$(cd "$out" && pwd)
    printf '%s  %s\n' "$archive_sha256" "$cache/$archive_name.tar.gz" | shasum -a 256 -c -
    tar -xOzf "$cache/$archive_name.tar.gz" "$archive_name/wasm-tools" > "$out/wasm-tools"
    chmod +x "$out/wasm-tools"
    cargo build --locked --offline --bin oddlang --bin publish-wasm-matrix
    compiler=$(cargo metadata --locked --offline --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"]+"/debug/oddlang")')
    ledger_key=$(printf %s 'urn:oddlang:ledger:wasm:package' | shasum -a 256 | cut -d ' ' -f 1)
    release_store="$out/cache"
    "$compiler" wasm publish contracts/wasm/ledger contracts/wasm/empty.lock.json "$release_store" "$out/ledger"
    mkdir -p "$release_store/wasm/$ledger_key"
    cp -R "$out/ledger" "$release_store/wasm/$ledger_key/1.0.0"
    "$compiler" wasm publish contracts/wasm/ledger-test contracts/wasm/ledger-test/dependencies.lock.json "$release_store" "$out/ledger-test"
    "$compiler" wasm publish contracts/wasm/echo contracts/wasm/empty.lock.json "$release_store" "$out/echo"
    "$(dirname "$compiler")/publish-wasm-matrix" "$out"
    WASM_TOOLS="$out/wasm-tools" scripts/build-wasm-guests.sh "$out" "$out/guests"
    cargo +"$rust_version" build --locked --offline --manifest-path examples/wasm/Cargo.toml \
        --target-dir target/wasm-host -p runtime-host
    cp target/wasm-host/debug/runtime-host "$out/runtime-host"
}

if [[ $# == 2 && $1 == --prepare ]]; then
    prepare "$2"
    exit
fi
if [[ $# != 0 ]]; then
    echo 'usage: scripts/verify-wasm.sh [--prepare EMPTY_DIRECTORY]' >&2
    exit 1
fi

root=$(mktemp -d "$cache/verification.XXXXXXXX")
for attempt in first second; do prepare "$root/$attempt"; done
for bundle in ledger ledger-test ledger-probe echo echo-label echo-rename echo-tags echo-import; do
    diff -qr "$root/first/$bundle" "$root/second/$bundle"
done
export ODDLANG_WASM_ARTIFACTS="$root/first"
export ODDLANG_WASM_HOST="$root/first/runtime-host"
export WASM_TOOLS="$root/first/wasm-tools"
cargo test --locked --offline --test wasm_runtime

scripts/test-wasm-js.sh "$root/first" "$root/hosts"
python3 - "$root" <<'PY'
from pathlib import Path
import hashlib, json, sys
root = Path(sys.argv[1])
def collect(attempt):
    base = root / attempt / "guests"
    return {str(p.relative_to(base)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(base.rglob("*")) if p.is_file() and p.suffix in {".wasm", ".rs"}}
first, second = collect("first"), collect("second")
if not first or first.keys() != second.keys():
    sys.exit("guest outputs differ between clean builds")
report = {name: {"first": digest, "second": second[name], "identical": digest == second[name]}
          for name, digest in first.items()}
(root / "reproducibility.json").write_text(json.dumps(report, indent=2))
if any(not row["identical"] for name, row in report.items() if name.endswith(".rs")):
    sys.exit("generated Rust differs between clean builds")
hosts = {}
for receipt in sorted((root / 'hosts').glob('*.json')):
    data = json.loads(receipt.read_text())
    hosts[receipt.stem] = {'caseCount': len(data['cases']), **{
        key: data[key] for key in ('host', 'version', 'componentSha256', 'releaseSha256', 'jcoVersion')
    }}
summary = {'hosts': hosts, 'builds': {name: row for name, row in report.items() if name.endswith('.wasm')}}
def observations(guest):
    return json.loads((root / 'first' / 'wasmtime-reports' / (guest + '.json')).read_text())['cases']
summary['trap'] = next({'case': c['caseId'], 'call': call}
                       for c in observations('trap') for call in c['calls'] if 'trap' in call)
repeated = observations('double-import')[0]
summary['doubleImport'] = {'case': repeated['caseId'], 'observedFunctions': [i['function'] for i in repeated['imports']]}
boundary = next(c for c in observations('max-exclusive') if c['caseId'] == 'exact-destination-headroom')
summary['upperEndpoint'] = {'case': boundary['caseId'], 'call': boundary['calls'][0]}
(root / 'runtime-observations.json').write_text(json.dumps(summary, indent=2) + '\n')

PY
echo "Verified Wasmtime, Node and Chromium evidence in $root"
