#!/usr/bin/env bash
set -euo pipefail
upstream=$1
out=$2
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
cargo build --locked --offline --bin oddlang
bin=$(cargo metadata --locked --offline --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"]+"/debug/oddlang")')
mapping=$root/examples/marmish/framing.toml
project() {
    local manifest=$1 import=$2 projection=$3
    scripts/marmish/offline.sh "$bin" import rust "$manifest" "$mapping" "$import"
    "$bin" import verify "$import"
    scripts/marmish/offline.sh "$bin" import replay "$manifest" "$mapping" "$import"
    "$bin" build module "$import" "$projection"
    "$bin" import verify-build "$import" "$projection"
}
project "$upstream/crates/marmish-core/Cargo.toml" "$out/import" "$out/projection"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
git -c init.templateDir= clone --quiet --no-local --no-checkout "$upstream" "$scratch/mutant"
git -C "$scratch/mutant" -c core.hooksPath=/dev/null checkout --quiet --detach 49fc705408a230bd0ad896248d08dbdd2faad05a
python3 - "$scratch/mutant/crates/marmish-core/src/frame.rs" <<'MUTATE'
from pathlib import Path
import sys
p = Path(sys.argv[1])
source = p.read_text()
version = """        if envelope.version != 1 {
            return Err(FrameError::InvalidVersion);
        }
"""
size = """        let max_encoded = Limits::RAW_BYTES.div_ceil(3) * 4;
        if envelope.payload.len() > max_encoded {
            return Err(FrameError::SizeLimit);
        }
"""
if source.count(version + size) != 1:
    raise SystemExit("could not identify both original guards")
p.write_text(source.replace(version + size, size + version))
MUTATE
project "$scratch/mutant/crates/marmish-core/Cargo.toml" "$out/mutant-import" "$out/mutant-projection"
scripts/marmish/frame.sh "$upstream" "$out/import" "$out/projection" "$out/observations" "$out/mutant-projection"
scripts/marmish/assembled-pilot.sh "$upstream" "$out" "$bin"
