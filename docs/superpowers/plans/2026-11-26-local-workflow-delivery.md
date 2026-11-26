# Verified local workflow delivery

**Goal** Express bounded queued, poll and consume handoffs with the ordinary native client without introducing a worker or execution fallback.

**Architecture** Extend a workflow with checked decoded replies, argument and capture comparisons, canonical nested request hashes and ordered binary outputs. Reply byte limits, digest bindings and declared state conditions precede any output. The companion graph specifies the local owner messages and their stable identities. An independent local fixture supplies a synthetic owner response, compares receipt bytes and checks that invalid or unbound output is never consumed. Authentication and policy enforcement stay with the declared owner role.

- [x] Freeze the original local queued, poll and consume behavior using the exact pinned client and the fixed binary fixture.
- [x] Verify reusable checked embedded replies and ordered binary output with a workflow fixture that has no Marmish role.
- [ ] Integrate the bounded generated Marmish client handoff and compare output, delivery cursors and failure behavior against the independent reference.
