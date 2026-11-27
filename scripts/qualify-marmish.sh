#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
mode=${1:-}
case "$mode" in
    baseline|frame|rebuild|verify) ;;
    *) echo 'usage: scripts/qualify-marmish.sh {baseline|frame|rebuild|verify} [UPSTREAM [EMPTY_OUTPUT]]' >&2; exit 2 ;;
esac
upstream=${2:-${ODDLANG_MARMISH_SOURCE:-$root/target/marmish/upstream}}
mkdir -p target/marmish
if [[ $# == 3 ]]; then
    mkdir -p "$3"
    report=$(cd "$3" && pwd)
    [[ -z "$(ls -A "$report")" ]] || { echo 'Output directory must be empty' >&2; exit 1; }
elif [[ $# -le 2 ]]; then
    report=$(mktemp -d "$root/target/marmish/$mode.XXXXXXXX")
else
    exit 2
fi
upstream=$(cd "$upstream" && pwd)
revision=49fc705408a230bd0ad896248d08dbdd2faad05a
[[ $(git -C "$upstream" rev-parse HEAD) == "$revision" ]] || { echo 'Marmish revision mismatch' >&2; exit 1; }
git -C "$upstream" diff --quiet HEAD -- || { echo 'Marmish tracked inputs differ from the pinned tree' >&2; exit 1; }
[[ -z "$(git -C "$upstream" ls-files --others --exclude-standard)" ]] || { echo 'Untracked Marmish inputs present' >&2; exit 1; }
export CARGO_NET_OFFLINE=true RUSTUP_AUTO_INSTALL=0 UV_OFFLINE=1 PYTHONDONTWRITEBYTECODE=1
if [[ $mode == rebuild ]]; then
    python3 -B scripts/marmish/rebuild.py "$upstream" "$report"
    echo "Marmish twice-rebuilt candidate in $report"
    exit 0
elif [[ $mode == verify ]]; then
    exec cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-phase7 -- "$upstream" "$report"
fi
if [[ $mode == frame ]]; then
    scripts/marmish/project-frame.sh "$upstream" "$report"
    echo "Marmish framing observations recorded in $report"
    exit 0
fi
scripts/marmish/offline.sh cargo +1.97.1 run --locked --offline --manifest-path verification/marmish-inventory/Cargo.toml -- \
    "$upstream" "$report/inventory" --compare-summary examples/marmish/inventory-summary.json
scripts/marmish/offline.sh python3 scripts/marmish/observations.py "$upstream" "$report/observations"
python3 scripts/marmish/baseline.py "$upstream" "$report/native"
cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-receipt -- check "$report" --write
cargo +1.97.1 run --quiet --offline --locked --manifest-path verification/marmish-inventory/Cargo.toml --bin marmish-receipt -- check "$report"
echo "Marmish baseline recorded in $report"
