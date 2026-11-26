# Checked local progress verification

The independent field-meter replay starts stdout at a nonzero confirmed cursor
and stderr at zero. It compares fixed binary streams and the ordered
acknowledgement. Wrong progress and output beyond the declared upper cursor
fail before any output. A nonmatching envelope tag prevents embedded paths
from matching. The legacy field-meter replay still uses member-only paths and
scalar literals.

```sh
python3 -B verification/local-owner-workflow/delivery.py "$ODDLANG"
python3 -B verification/local-owner-workflow/check.py "$ODDLANG"
```

Both replays and eighteen release library tests passed with Rust 1.97.1. Both
work graph Turtle files parsed and the ready-work query ran. Claude Code auto
review approved prior cursor checks, authority before owner contact, native
decoder linkage, unique timestamped identities and unchanged legacy policy
order. Fresh submission uses a zero origin in the companion graph. Inline
resume reads the previously acknowledged cursor after validating the stored
profile and explicit handle.

The source-denied macOS ARM64 [companion build][build] compiled both composed
executables. The native client matched all eight [mainline local cases][oracle]
for exact frame order, stream bytes and exits. Cases cover two polls, a
sixteen-poll delivery of the fixed 128 KiB streams, detached handle formatting,
inline wait at zero and nonzero cursors, separate Finish acknowledgement,
corrupt response rejection and command denial. The [candidate receipt][client]
also measures four handle or manifest rejections, nonzero fresh cursors and a
digest-valid response with the wrong session.

Authenticated peer command delivery and restart during a pending poll have
their own acceptance conditions.

[build]: https://github.com/Epiphytic/marmish-oddlang/blob/d3b1296165eff86f1f311a877e735486bbaa54d0/verification/whole/basic-command-interop/bounded-local-progress-build-aarch64-apple-darwin.json
[oracle]: https://github.com/Epiphytic/marmish-oddlang/blob/d3b1296165eff86f1f311a877e735486bbaa54d0/verification/whole/basic-command-interop/upstream-local-progress-aarch64-apple-darwin.json
[client]: https://github.com/Epiphytic/marmish-oddlang/blob/d3b1296165eff86f1f311a877e735486bbaa54d0/verification/whole/basic-command-interop/bounded-local-progress-candidate-aarch64-apple-darwin.json
