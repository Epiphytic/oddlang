# Bounded local owner workflows

> **For agentic workers:** Use superpowers:executing-plans and verify the independent socket witness.

**Goal** Generate a normal native CLI handler that drives an ordered, bounded sequence of same-user Unix request/reply exchanges.

**Architecture** The workflow graph assigns stable IRIs to states, request fields and transitions. Requests have ordered JSON fields that can use literal values, explicit linked CLI arguments, and validated values captured from previous replies. Each state evaluates ordered response predicates, transfers newly captured or explicitly carried values, and moves to a declared state or exits with declared diagnostics. One absolute deadline and exchange budget cover retries. The framework projects the same-user framed socket effect. A companion contract supplies the Marmish operation names, field order, reply policies and future transport outcomes.

**Scope** The first increment implements bounded readiness, preparation, and dispatch in the reusable primitive. Export strict JSON parsing and the stateless request wrapper as reusable source pieces with unchanged HTTP and stateless behavior. Initial Marmish integration consumes separately qualified profile and target admission. It needs byte-oriented payload and invocation binding plus polling and durable cursor handling before it can claim command completion.

- [x] Freeze actual upstream readiness, preparation, and submit IPC and verify a malformed or unexpected response fails closed.
- [x] Verify the reusable sequencer through an unrelated ordinary CLI entry and independent Unix peer. Exercise capture validation, a same-UID connection, exact framing, explicit arguments, retries, ordered transition precedence, and exchange limits.
- [x] Record the scope and verification and review in Claude auto mode.

[Native verification](../../../verification/local-owner-workflow/runtime-verification.md)
