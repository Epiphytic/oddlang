---
name: oddlang-import-cleanup
description: Use when Oddlang rejects a Rust import at a source span, a pinned identity mapping is ambiguous, or a reviewer needs to assess a proposed cleanup against the original crate.
---

# Reviewing a Rust import cleanup

Start with the exact importer diagnostic, byte-for-byte original crate, selected source span, pinned mapping and overlays. Record the diagnostic and original build configuration. Retain the unmodified crate as the executable comparison baseline. Create a separate editable copy. Run offline with the pinned toolchain. Invoke this skill only for a requested proposal.

For an unsupported guard macro, inspect its definition, expansion at the selected call, name resolution, evaluation count, and early return. Propose an equivalent explicit guard in the same position only when the expansion establishes that equivalence. For a duplicate guard IRI, show both declarations and request the mapping owner's intended identities. Guard order and similar names alone cannot establish identity. For a recurring unresolved macro, derive, trait or library construct, record the source span, blocked construct and examples for a future compiler-backed frontend.

Generate the proposal with Python 3.9+ and Git. Keep the original, edited copy and output in separate directories. The helper captures regular files other than top-level Cargo build output and Git metadata. Its patch can change existing Rust sources or the chosen mapping. Supply relative paths using `--manifest` and `--mapping` if needed.

```sh
python3 skills/oddlang-import-cleanup/scripts/cleanup.py propose original edited review
```

Give the reviewer the diagnostic, source span, mapping evidence, review/cleanup.patch and the printed SHA-256 of review/proposal.json. Explain every change to guard priority, operand evaluation and observable state. Compare the original and edited executables on overlapping failures. For example, an absent source and negative amount must still return SourceMissing. A mismatch disproves equivalence even if both crates import or compile. Withhold an equivalence claim and revise the proposal.

For a guard macro, inspect its definition and substitute the same predicate and operands at the same statement. For two declarations mapped to one IRI, ask the contract owner for identity evidence and retain both candidates until they answer. When repeated failures require trait, macro or type resolution, record the construct and span for a future compiler-backed frontend.

Wait for explicit acceptance of that exact digest. A new patch, mapping or source snapshot requires a fresh review. After acceptance, substitute the reviewed digest below and re-import the archived effective input.

```sh
python3 skills/oddlang-import-cleanup/scripts/cleanup.py accept review --sha256 "$REVIEWED_DIGEST"
oddlang import rust review/effective/Cargo.toml review/effective/mapping.toml imported --accepted-cleanup review/accepted.json
oddlang import replay review/effective/Cargo.toml review/effective/mapping.toml imported --accepted-cleanup review/accepted.json
```

Keep the entire review directory alongside import.lock. Re-import and replay check the original tree, effective tree and actual patch bytes. Use review/original for the phase 4 executable comparison. Offline import and replay consume these stored inputs without consulting an assistant.
