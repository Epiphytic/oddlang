# Contract recovery across languages

Oddlang recovers the structure and behavior needed to implement a compatible
artifact. The authoritative model lives in a versioned ontology. It has stable
identities for components, roles, interfaces, operations, data types, and
dependencies on other contracts. It explicitly states the rules consumers
depend on, including data constraints, state transitions, errors, wire formats,
and observable effects. A target can choose its own implementation structure,
language, framework, and native libraries.

## What counts as compatible

| Boundary | Evidence |
| --- | --- |
| Data schema or protocol | Published fields, constraints, encodings, errors, and the applicable reader and writer directions |
| Operation or service | Declared results and effects for the same input and starting state, including error and resource boundaries specified by the contract |
| Drop-in executable or library | Applicable CLI, ABI, wire, platform, and calling or deployment expectations for the artifacts being compared |

Canonical contract serialization gives a repeatable release pin. Each
implementation is qualified against the same pinned obligations and
independently established expected outcomes. Where applicable, test original
and replacement as mixed peers in both directions. Publish the role, version,
configuration, covered obligations, and gaps with each claim.

Exact bytes matter where the contract specifies them. A frame tag or binary
field layout can be normative on the wire. Exact Rust text and binary hashes
are separate measures of source and build fidelity.

## Marmish example

A stream contract would name the Data frame, assign its tag, describe its
fields and limits, and specify decoding errors and window transitions.
For example, a Data frame uses tag `5` and carries an offset and bounded
payload. Implementations can use the same contract and wire cases while
choosing different buffer, parser, or compression libraries. Recover each
rule from reviewed source, specifications, or observations, and record
unresolved behavior as a coverage gap. The existing independent wire corpus
is candidate verification evidence. Its cases do not by themselves supply a
complete normative contract.

The completed syntax-graph experiment isolated and regenerated five Rust
files. It measured reconstruction fidelity and supplied some reusable
behavioral evidence through the wire corpus. This decision supersedes the
follow-on estimate for sharding those syntax graphs across the remaining
Marmish workspace. Future coverage is measured by recovered declarations and
discharged compatibility obligations. [Experiment report](https://github.com/Epiphytic/oddlang/blob/e5af0cc48e386806a21a54765ef9318284b281be/docs/design/marmish-stream-experiment.md)
