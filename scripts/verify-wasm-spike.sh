#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source examples/wasm/toolchain.lock
export CARGO_NET_OFFLINE=true
export RUSTUP_AUTO_INSTALL=0
rustc +"$rust_version" -Vv | grep -Fx "commit-hash: $rust_commit"
printf '%s  %s\n' "$archive_sha256" "$cache/$archive_name.tar.gz" | shasum -a 256 -c -
out=$(mktemp -d "$cache/spike.XXXXXXXX")
tar -xzf "$cache/$archive_name.tar.gz" -C "$out" "$archive_name/wasm-tools"
wasm_tools="$out/$archive_name/wasm-tools"
manifest=examples/wasm/Cargo.toml
cargo +"$rust_version" build --locked --offline --manifest-path "$manifest" --target-dir "$out/guest" \
    --target "$guest_target" --profile "$guest_profile" -p pure-echo
component="$out/guest/$guest_target/$guest_profile/pure_echo.wasm"
"$wasm_tools" validate "$component"
"$wasm_tools" component wit "$component" -o "$out/pure-echo.wit"
diff -u examples/wasm/evidence/pure-echo.wit "$out/pure-echo.wit"
cargo +"$rust_version" build --locked --offline --manifest-path "$manifest" --target-dir target/wasm-host -p empty-linker
{
    rustc +"$rust_version" --version
    "$wasm_tools" --version
    echo "guest target = $guest_target ($guest_profile)"
    echo "component sha256 = $(shasum -a 256 "$component" | cut -d ' ' -f 1)"
    echo "extracted WIT sha256 = $(shasum -a 256 "$out/pure-echo.wit" | cut -d ' ' -f 1)"
    target/wasm-host/debug/empty-linker "$component"
} | tee "$out/observations.txt"
# Compile non-executing structural probes for each published world's canonical ABI.
cargo +"$rust_version" build --locked --offline --bin oddlang --target-dir target/wasm-publisher
release_cache="$out/cache"
ledger_key=$(printf %s 'urn:oddlang:ledger:wasm:package' | shasum -a 256 | cut -d ' ' -f 1)
for fixture in ledger ledger-test echo; do
    world=$fixture
    if [[ $fixture == echo ]]; then world=echo-world; fi
    lock=contracts/wasm/empty.lock.json
    if [[ $fixture == ledger-test ]]; then
        lock=contracts/wasm/ledger-test/dependencies.lock.json
        mkdir -p "$release_cache/wasm/$ledger_key"
        cp -R "$out/ledger" "$release_cache/wasm/$ledger_key/1.0.0"
    fi
    target/wasm-publisher/debug/oddlang wasm publish "contracts/wasm/$fixture" "$lock" \
        "$release_cache" "$out/$fixture"
    "$wasm_tools" component embed --dummy --world "$world" "$out/$fixture/wit" -o "$out/$fixture-core.wasm"
    "$wasm_tools" component new "$out/$fixture-core.wasm" -o "$out/$fixture.wasm"
    "$wasm_tools" validate "$out/$fixture.wasm"
    "$wasm_tools" component wit "$out/$fixture.wasm" -o "$out/$fixture.wit"
    diff -u "examples/wasm/evidence/$fixture.wit" "$out/$fixture.wit"
done
printf 'Spike evidence: %s\n' "$out"
