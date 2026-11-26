#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_NET_OFFLINE=true
export RUSTUP_AUTO_INSTALL=0
mkdir -p target
if [[ $# == 0 ]]; then
    report=$(mktemp -d "$PWD/target/translation.XXXXXXXX")
elif [[ $# == 1 ]]; then
    mkdir -p "$1"
    report=$(cd "$1" && pwd)
    if [[ -n "$(ls -A "$report")" ]]; then
        echo 'Translation evidence needs an empty output directory.' >&2
        exit 1
    fi
else
    echo 'usage: scripts/verify-rust-translation.sh [EMPTY_OUTPUT_DIR]' >&2
    exit 1
fi
fixture=examples/rust-import/ledger
(cd "$fixture" && shasum -a 256 -c original.sha256)
python3 skills/oddlang-import-cleanup/scripts/cleanup.py freeze "$fixture" "$report/original.pin"
cmp examples/rust-import/ledger.baseline "$report/original.pin"
cargo build --locked --offline --bin oddlang
bin=$(cargo metadata --locked --offline --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"]+"/debug/oddlang")')
"$bin" import rust "$fixture/Cargo.toml" "$fixture/mapping.toml" "$report/import"
"$bin" import replay "$fixture/Cargo.toml" "$fixture/mapping.toml" "$report/import"
"$bin" build "$report/import" "$report/generated"
"$bin" import verify-build "$report/import" "$report/generated"
ODDLANG_TRANSLATION_ARTIFACTS="$report/corpus" ODDLANG_TRANSLATION_REPORT="$report/differential.json" \
    cargo test --locked --offline --test rust_translation
original_sha=$(awk '$2=="src/lib.rs" {print $1}' "$fixture/original.sha256")
generated_sha=$(shasum -a 256 "$report/generated/account.rs" | cut -d ' ' -f 1)
verification/rust-translation/scripts/prove.sh "$fixture/src/lib.rs" "$report/generated/account.rs" \
    "$original_sha" "$generated_sha" "$report/proof"
python3 verification/translation-evidence/verify.py "$PWD" "$report" --write
python3 verification/translation-evidence/verify.py "$PWD" "$report"
ODDLANG_REPLAY_FIXTURE="$report" python3 -B -m unittest discover -s verification/translation-evidence
echo "Verified Rust translation evidence in $report"
