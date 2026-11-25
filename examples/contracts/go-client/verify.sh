#!/bin/sh
set -eu
command -v go >/dev/null || { echo 'Go 1.22+ is required for the offline adoption fixture' >&2; exit 1; }
fixture=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
contracts="$fixture/../../../contracts"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export GOENV=off GOWORK=off GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local GOFLAGS=-mod=readonly
go build -o "$scratch/replay" "$fixture/tools/replay.go"
for consumer in publisher client; do
    mkdir "$scratch/$consumer"
    cp "$fixture/go.mod" "$fixture/client_test.go" "$fixture/lock_test.go" "$scratch/$consumer/"
done

install_release() {
    cd "$scratch/$1"
    cp "$2" client.lock.json
    cp "$3" target.lock.json
    version=$("$scratch/replay" restore "$fixture/revisions")
    rm -rf bundle
    cp -R "$contracts/cache/ledger/$version" bundle
}
native_test() {
    cd "$scratch/$1"
    "$scratch/replay" verify >/dev/null
    EXPECTED_VERSION="$2" go test -count=1 client.go client_test.go lock_test.go
}
for v in v1 v2; do
    install_release publisher "$contracts/workspace-$v.lock.json" "$fixture/pins/target-$v.lock.json"
    native_test publisher "$v"
    if [ "$v" = v1 ]; then
        install_release client "$fixture/pins/client-v1.lock.json" "$fixture/pins/target-v1.lock.json"
    fi
    native_test client v1
done
install_release client "$fixture/pins/client-v2.lock.json" "$fixture/pins/target-v2.lock.json"
native_test client v2
printf '\n// changed after review\n' >> client.go
if "$scratch/replay" verify > "$scratch/rejection" 2>&1; then exit 1; fi
grep -F 'digest mismatch for client.go' "$scratch/rejection"
"$scratch/replay" restore "$fixture/revisions"
printf 'package client\n' > extra.go
if "$scratch/replay" verify > "$scratch/rejection" 2>&1; then exit 1; fi
grep -F 'unreviewed source extra.go' "$scratch/rejection"
rm extra.go
native_test client v2
