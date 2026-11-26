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

The companion qualification covers the native role wiring and pinned Marmish
local oracle. Authenticated peer command delivery and process restart during a
pending poll have their own acceptance conditions.
