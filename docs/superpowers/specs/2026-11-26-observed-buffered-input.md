# Observed buffered-input policies

Use a fresh initially exhausted specimen for each trial. Each provider
supplies its declared distinct nonzero payload on the first call. Payloads
contain at least two octets and fit the recovered capacity.

- `emptyRead` polls into an empty destination, then calls fill. The returned
  count, provider call counts and first visible payload distinguish early
  return from a refill on exhaustion.
- `consume` fills, partially consumes the declared positive count, observes
  the remaining suffix, consumes the rest, then refills into an EOF.
  The provider captures the initialized storage prefix before writing.
  The prefix distinguishes erase from retain among the declared policies.
- `refillError` fills and exhausts the specimen. The provider then returns
  Interrupted without writing and supplies EOF if called again. The
  error, retry view and call counts distinguish retained old extent from
  discard. A retained view must also agree with the inferred erase policy.

The classifier constrains the existing policy family. These observations
cover the named trials. Static method recovery, exact erase timing, other
fault sequences and general trace equivalence require separate evidence. The
storage provider remains responsible for erase on drop.

The JSON trace format is `oddlang-buffered-input-trace-v1`. Each trial has
a `payload` octet array. The remaining fields are listed below.

| Trial | Fields |
| --- | --- |
| `emptyRead` | `returned`, `callsAfterEmpty`, `afterFill`, `callsAfterFill` |
| `consume` | `requested`, `afterPartial`, `callsAfterPartial`, `nextReadPrefix`, `afterRefill`, `callsAfterRefill` |
| `refillError` | `returnedError`, `callsAfterError`, `afterRetry`, `callsAfterRetry` |

The return count, requested count and call counts are nonnegative
integers. The `after*` and prefix fields contain octet arrays.
`returnedError` is true when the expected Interrupted propagated.
