# Authorized local command inputs

> **For agentic workers:** Use superpowers:executing-plans and qualify the generated client against the independent local owner witness.

**Goal** Let a generated local workflow resolve an authorized external destination and preserve its argv and stdin through the existing native CLI entry.

**Architecture** The local workflow may select a Rust profile input provider in its target binding. The portable input graph declares the private document fields, roles, trusted accounts, allowed relay and route combinations, payload bounds, diagnostics and named initial values. The provider reads the profile with descriptor-relative no-follow checks and never writes it. It validates explicit target, route and relay choices before selecting or contacting the owner socket. The generated workflow retains those input values for the invocation and uses existing captured values for state transitions.

**Scope** The companion graph supplies explicit-state, no-autostart external destination handling for the authorized local status, prepare and submit handoff. Pinned benign fixtures run against an owned Unix socket in a private network-denied workspace. No worker or shell execution is introduced by this input provider. Handling queued work, polling, consume and peer-side completion remain separate work.

- [ ] Attach a profile and discovered endpoint to the generic workflow without changing the existing stateless projections.
- [ ] Generate the normal Marmish client from the composed candidate, compare exact local owner frames and refusals with the pinned reference, and reject altered authority, profile permissions and relay configuration before owner contact.
- [ ] Review with Claude auto mode and record native evidence by contract and role.
