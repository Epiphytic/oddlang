#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
mode=${1:-}
case "$mode" in
    baseline) ;;
    *) echo 'usage: scripts/qualify-marmish.sh baseline [UPSTREAM [EMPTY_OUTPUT]]' >&2; exit 2 ;;
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
scripts/marmish/offline.sh python3 scripts/marmish/inventory.py "$upstream" "$report/inventory"
python3 - "$report/inventory/inventory-summary.json" <<'CHECK_INVENTORY'
import json, sys
from pathlib import Path
expected = json.loads(Path("examples/marmish/inventory-summary.json").read_bytes())
actual = json.loads(Path(sys.argv[1]).read_bytes())
host_specific = {"host", "inventorySha256"}
def portable(summary):
    return {key: value for key, value in summary.items() if key not in host_specific}
if portable(expected) != portable(actual):
    raise SystemExit("portable Marmish inventory or summary changed")
if expected["host"] == actual["host"] and expected != actual:
    raise SystemExit("host Marmish inventory changed")
CHECK_INVENTORY
scripts/marmish/offline.sh python3 scripts/marmish/observations.py "$upstream" "$report/observations"
python3 scripts/marmish/baseline.py "$upstream" "$report/native"
python3 scripts/marmish/receipt.py "$report" --write
python3 scripts/marmish/receipt.py "$report"
echo "Marmish baseline recorded in $report"
