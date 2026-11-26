# Typed collection and text guards

Extend the closed wire protocol step and expression vocabulary. A list traversal creates a lexical scope for each borrowed item, preserves order, and stops at the first guard failure. A byte length on bounded base64 uses the decoded native bytes. Text compares exact UTF-8, can require a literal prefix, and can exclude NUL. Canonical JSON string length counts UTF-8 bytes, enclosing quotes and serializer escapes in one allocation-free pass.

- [x] Derive typed Rust scalar guards and compile nested list scopes.
- [x] Verify an unrelated weather package on encode and decode and reject a non-list traversal, scope shadowing, mistyped text checks and nested duplicate guard identities.
