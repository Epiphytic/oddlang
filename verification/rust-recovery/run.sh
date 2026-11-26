#!/usr/bin/env bash
set -euo pipefail

if (( $# < 6 )); then
    echo 'usage: run.sh MANIFEST PACKAGE lib|bin TARGET TRIPLE default|no-default [FEATURE ...]' >&2
    exit 2
fi

manifest=$1
package=$2
target_kind=$3
target_name=$4
target_triple=$5
default_mode=$6
shift 6

case "$target_kind" in
    lib) target=(--lib) ;;
    bin) target=(--bin "$target_name") ;;
    *) echo "unsupported Cargo target kind: $target_kind" >&2; exit 2 ;;
esac

case "$default_mode" in
    default) ;;
    no-default) target+=(--no-default-features) ;;
    *) echo "invalid default feature mode: $default_mode" >&2; exit 2 ;;
esac

for feature in "$@"; do
    target+=(--features "$feature")
done

adapter_dir=$(cd "$(dirname "$0")" && pwd)
toolchain=1.97.1
expected_commit=8bab26f4f68e0e26f0bb7960be334d5b520ea452
rustc=$(rustup which --toolchain "$toolchain" rustc)
cargo=$(rustup which --toolchain "$toolchain" cargo)
compiler=$("$rustc" -vV)
compiler_commit=$(printf '%s\n' "$compiler" | sed -n 's/^commit-hash: //p')
host=$(printf '%s\n' "$compiler" | sed -n 's/^host: //p')
if [[ "$compiler_commit" != "$expected_commit" || -z "$host" ]]; then
    echo "unexpected rustc build for $toolchain" >&2
    exit 1
fi
sysroot=$("$rustc" --print sysroot)
output=$(mktemp -d "${TMPDIR:-/tmp}/oddlang-rust-recovery.XXXXXXXX")
trap 'rm -rf "$output"' EXIT

# Cargo profile and flag variables can add cfg branches without changing the
# requested Cargo features. Cargo project configuration still participates.
unset RUSTFLAGS RUSTDOCFLAGS RUSTC_BOOTSTRAP RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset CARGO_ENCODED_RUSTFLAGS CARGO_ENCODED_RUSTDOCFLAGS CARGO_BUILD_TARGET
unset CARGO_BUILD_RUSTFLAGS CARGO_BUILD_RUSTDOCFLAGS CARGO_BUILD_RUSTC_WRAPPER
unset CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER
while IFS= read -r variable; do
    case "$variable" in
        CARGO_PROFILE_*|CARGO_TARGET_*_RUSTFLAGS|CARGO_TARGET_*_RUSTDOCFLAGS)
            unset "$variable"
            ;;
    esac
done < <(compgen -e)

# rustc_private is enabled for the adapter crate only. The requested crate is
# compiled as normal by the pinned stable compiler.
RUSTC="$rustc" RUSTC_BOOTSTRAP=oddlang_rust_recovery_driver \
    RUSTFLAGS="-C rpath=yes" \
    "$cargo" build --quiet --locked --offline \
    --manifest-path "$adapter_dir/Cargo.toml" --target-dir "$output/driver" --target "$host" >&2

cd "$(dirname "$manifest")"
verbosity=(--quiet)
if [[ ${ODD_RECOVERY_PROFILE_TRACE:-0} == 1 ]]; then
    verbosity=(-vv)
    # Both observers use this same virtual source root during reconciliation.
    export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$(pwd -P)=/oddlang-workspace"
fi
RUSTC="$rustc" \
    RUSTC_WORKSPACE_WRAPPER="$output/driver/$host/debug/oddlang-rust-recovery-driver" \
    ODD_RECOVERY_PACKAGE="$package" ODD_RECOVERY_TARGET="$target_name" \
    ODD_RECOVERY_KIND="$target_kind" ODD_RECOVERY_OUT="$output/inventory.json" \
    ODD_RECOVERY_SYSROOT="$sysroot" ODD_RECOVERY_COMPILER="$compiler_commit" \
    "$cargo" check "${verbosity[@]}" --locked --offline --release \
    --manifest-path "$manifest" --target-dir "$output/target" \
    --target "$target_triple" --package "$package" "${target[@]}" >&2
cat "$output/inventory.json"
