# Local provider lifecycle implementation plan

**Goal** Generate an owned foreground provider lifetime for a prepared direct file profile, with exact local activation, termination and startup-error observations.

**Design** Keep the existing foreground status contract and add an optional lifecycle declaration. A verified binding selects the already-pinned file-profile store semantics and a native provider constructor. The constructor opens retained encrypted storage handles and an account device with an actual relay SDK client and notification task. It runs only on the single explicit loopback TLS endpoint selected by the foreground contract. A terminal report maps each observation to an actual operation, or to the explicit absence of the engine for this role. All handles are dropped before runtime cleanup and before releasing the exclusive file lease.

The portable shutdown contract declares the report field identities, stable order, version, reason mapping and bounded terminal stages. The Rust provider adapter binds native types and methods. Activation cancels through the scoped transport, and any timeout or failed join stays false in the report. A preflight failure before listener publication returns its named error and preserves the owned binding for a subsequent retry. Interoperation evidence covers a pristine current-format prepared profile through a held or rejected direct TLS attempt. WebSocket messaging, readiness, nonempty histories, configured execution and installed services lie outside this phase.

- [x] Observe the pinned original on held TLS SIGINT and SIGTERM, rejected certificate and both corrupted and unsafe store starts. Measure owned runtime, lease and durable state before/after.
- [x] Make an unrelated delayed/failing provider fixture test/report each actual stage outcome and preserve existing output when no lifecycle binding is selected.
- [x] Compose checked file-profile store bindings with the genuine encrypted journal, account device, relay SDK and its joined notification forwarding task. Emit the checked report only after ownership barriers complete.
- [x] Qualify original and generated peers against separate held loopback listeners and check provider state before and after. Verify source-isolated builds on both ARM64 hosts. Publish reviewed framework and companion artifacts.

[Native ARM64 qualification](https://github.com/Epiphytic/marmish-oddlang/pull/53)
