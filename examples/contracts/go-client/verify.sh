#!/bin/sh
set -eu
fixture=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
contracts="$fixture/../../../contracts"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/client" "$scratch/publisher"
cd "$scratch/client"
cp "$fixture/go.mod" "$fixture/"*_test.go .
cp -R "$fixture/revisions" .
export GOENV=off GOWORK=off GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local GOFLAGS=-mod=readonly
install_release() {
    cp "$fixture/pins/client-$1.lock.json" client.lock.json
    cp "$fixture/pins/target-$1.lock.json" target.lock.json
    rm -rf bundle
    cp -R "$contracts/cache/ledger/$1" bundle
}
native_test() {
    EXPECTED_VERSION="$1" go test -count=1 .
}
cp "$contracts/workspace-v1.lock.json" "$scratch/publisher/workspace.lock.json"
install_release v1
cp revisions/ledger-1/client.go .
native_test v1
cp "$contracts/workspace-v2.lock.json" "$scratch/publisher/workspace.lock.json"
if cmp -s client.lock.json "$scratch/publisher/workspace.lock.json"; then exit 1; fi
native_test v1
install_release v2
native_test v2
printf '\n// changed after review\n' >> client.go
if native_test v2 > "$scratch/rejection" 2>&1; then exit 1; fi
grep -F 'digest mismatch for client.go' "$scratch/rejection"
cp revisions/ledger-1/client.go .
native_test v2
