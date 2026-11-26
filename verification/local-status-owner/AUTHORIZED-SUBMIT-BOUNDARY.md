# Authorized local submit boundary

The generated companion CLI and owner passed a cold offline Cargo check on macOS ARM64 with Rust 1.97.1. A separate network-denied test used a synthetic retained room and a keyed SQLCipher journal. The generated lookup rejected the wrong role, withdrawn trust, wrong relay, and wrong room. The commit persisted the independently encoded frame and digest. Reopening the journal returned the same receipt on exact retry, and reusing the request ID for a different frame was rejected.

The standalone durable queue fixture establishes process restart recovery. The local submit fixture establishes connection reopen and the local receipt boundary with synthetic retained state. Live native membership, Welcome evidence and peer delivery remain outside this result. A timeout can lose a reply after commit. An exact retry uses the original request ID.

## Graph-selected receipt

The companion foreground graph has SHA-256 `c5e1070f69d987ea84ac11781aa14b424654d091eaded9a5ebaca3c8ba8dbc14`. It declares `accepted_ok = true`, `refused_ok = true` and `handle_format = 1`. The regenerated CLI and owner built offline with pinned dependencies. The network-denied test `local_room_selection_and_commit_receipt` passed against the keyed journal. Both unprepared CLI preflight cases also passed with network access denied. [Pinned companion graph](https://github.com/Epiphytic/marmish-oddlang/blob/9b0e86eee429e5c39cf448ef3394950cea581f59/oddlang/portable/authorized-owner-submit-v1/foreground.json)

All 19 compiler library tests passed. The alternate gauge graph exercises different request, reply and receipt spellings, `refused_ok = false` and receipt version 7.
