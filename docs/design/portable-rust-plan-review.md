# Claude review of the portable Rust recovery plan

Claude Code 2.1.332 reviewed the plan and contract recovery design using its CLI in auto permission mode. It was asked to identify blocking omissions in the plan. The author incorporated the following five findings before publication.

| Finding | Change to the plan |
| --- | --- |
| The private inventory leaks library choices, and Rust callers need a normative public signature. | Give the Rust API binding its own digest and compile probes. Withhold private structure from both target builds. |
| The vocabulary arrives too late and could encode a program as ordered facts. | Define structural and semantic vocabularies before extraction, limit edge granularity and rule forms, and reject four concrete payload classes. |
| Process I/O needs explicit semantics. | Specify channel type, partial reads, EOF, flushes, failure behavior, output order and host inputs for each CLI claim. |
| The Node compatibility claim lacks a role boundary. | Give each target named roles and limit Rust source compatibility to the Rust binding. |
| Library-shaped outputs can accidentally freeze an implementation. Equivalent graphs need a stable pin. | Pin role contracts and equivalence rules, demonstrate alternative valid bytes, and check canonical graph digests across fresh extractions and reordered triples. |

These are plan revisions. The phase gates specify the runtime evidence to collect during implementation.
