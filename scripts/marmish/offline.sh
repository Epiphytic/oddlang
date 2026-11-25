#!/usr/bin/env bash
set -euo pipefail
case $(uname -s) in
    Darwin)
        exec sandbox-exec -p '(version 1)(allow default)(deny network-outbound (remote ip "*:*"))(deny network-inbound (local ip "*:*"))' "$@"
        ;;
    Linux)
        exec unshare --user --map-root-user --net -- "$@"
        ;;
    *) echo 'No network-isolated verifier for this host' >&2; exit 1 ;;
esac
