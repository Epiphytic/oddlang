#!/usr/bin/env bash
set -euo pipefail
upstream=$1
out=$2
bin=$3
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
python3 -B scripts/marmish/prepare.py "$upstream" "$out/import" "$out/projection" "$scratch/package"
manifest=$scratch/package/project.toml
store=$scratch/package/store
candidate=$scratch/candidate
"$bin" assemble "$manifest" "$store" "$candidate"
cp "$manifest" "$out/assembly.toml"
scripts/marmish/offline.sh cargo +1.97.1 test --locked --offline --manifest-path "$candidate/Cargo.toml" --target-dir "$scratch/native-tests" -p marmish-core --test contracts >"$out/upstream-contracts.log" 2>&1
python3 -B scripts/marmish/check_build_closure.py "$candidate" "$manifest" "$out/build-closure"
# Retain the previously authorized manifest while substituting bytes at a retained address.
hash=$(git -C "$upstream" show 49fc705408a230bd0ad896248d08dbdd2faad05a:crates/marmish-core/src/config.rs | shasum -a 256 | cut -d ' ' -f 1)
printf '\n' >>"$store/retained/$hash"
if "$bin" assemble "$manifest" "$store" "$scratch/poisoned" >"$out/poisoned-retained.log" 2>&1; then
    echo 'Assembler accepted a changed retained config module' >&2
    exit 1
fi
[[ ! -e "$scratch/poisoned" ]]
