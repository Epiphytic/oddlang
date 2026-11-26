# Deferred output after local cursor lookup

A local workflow can carry a bounded JSON object from a checked reply to a
later step. That step can compare declared sources after capturing its own
reply. It validates both binary streams and their cursor bounds before
output. A failed binding check prevents output and the next local exchange.

The independent field-meter fixture checks deferred output after a cursor
lookup, a wrong request binding, and a changed cursor. The normal fixture
exercises output from the current reply and the existing plain workflow
checks compatibility without the delivery binding.

```sh
python3 -B verification/local-owner-workflow/delivery.py "$ODDLANG" --deferred
python3 -B verification/local-owner-workflow/delivery.py "$ODDLANG"
python3 -B verification/local-owner-workflow/check.py "$ODDLANG"
```

All three fixture runs passed. The deferred case rejected a matching request
hash paired with bad cursor arithmetic, an oversized retained object and a
zero object bound. The Rust 1.97.1 release library suite passed 19 tests after
integrating the authorized local submit compiler change. Both Turtle files
parsed and the ready-work query ran.
