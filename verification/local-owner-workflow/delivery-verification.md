# Checked local reply delivery verification

The field-meter fixture generates a native workflow and calls it through an
owned Unix socket. The independent peer checks the exact request bytes and
supplies a strict embedded reply, canonical request binding, separate binary
streams and zero-start delivery cursors. The client writes both streams,
requests acknowledgement and exits with the captured status. A failed
acknowledgement terminates with the declared protocol diagnostic.

Negative replies cover digest and request binding mismatches, wrong cursors,
missing array members, noncanonical Base64, an oversized decoded payload,
duplicate JSON members, a reserved peer field and an invalid exit status. Each
fails before stream output and acknowledgement. The prior field-meter
workflow also replays with the delivery binding omitted.

```sh
python3 -B verification/local-owner-workflow/delivery.py "$ODDLANG"
python3 -B verification/local-owner-workflow/check.py "$ODDLANG"
```

Claude Code auto-mode review found that array paths and composite comparison
literals needed explicit delivery selection. It also required the new rules
in the obligation catalog. The follow-up approved those corrections.

The current portable scope covers one validated response with streams starting
at cursor zero. Multi-poll presentation, detached handle formatting, resume,
peer transport and generated owner delivery remain separate checks.
