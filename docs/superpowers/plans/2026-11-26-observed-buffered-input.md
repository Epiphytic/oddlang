# Observed buffered-input policy recovery plan

**Goal** Infer the three existing private buffered-input policies from
controlled native observations while recovering the shape independently.

**Architecture** Each named stimulus uses a fresh buffer and a conforming
instrumented Read provider. A deterministic classifier accepts the
observations only when one member of each two-policy axis fits and the
axes agree. Shared shape recovery emits the existing graph and overlay.
Its receipt records structure, inferred policies and assumptions separately.

**Spec** [Observed buffered-input policy recovery](../specs/2026-11-26-observed-buffered-input.md)

- [x] Collect an unrelated real native trace and reproduce the missing recovery route.
- [x] Validate the three stimuli, reject inconsistent observations and preserve the authored route.
- [x] Change the unrelated native implementation, confirm the recovered shape is invariant and replay both inferred graphs.
- [ ] Review and publish the independent framework branch.
