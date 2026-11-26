# Rust field conjunction review

Claude CLI in auto mode reviewed the compiler observer and portable lowering. It identified stale proof when the selected file stayed unchanged while a field declaration or equality implementation changed elsewhere. It also identified hashing of normalized source and the possibility of selecting a trait implementation as an inherent method.

The observer now records and checks raw bytes for the compiler's local Rust source inventory and uses the compiler's inherent-implementation query. The independent fixture exercises a CRLF source file and rejects an old receipt after an equality implementation changes in a secondary module.

The follow-up review found a field index could belong to a custom dereference target, and a function-local type could share a rendered path with a module-level type. The observer now requires a single built-in shared-reference dereference, checks both the field index and its name, and resolves owner paths only through module ancestors. The fixture exercises an effectful dereference, a trait-only method, and aliasing and shadowing inside receiver expressions.

The final auto review found no blocking issues in receiver or owner resolution. The unrelated native fixture passed both source revisions, cross-file invalidation and six unresolved cases. The companion replay reproduced the published package lock from the archived framework revision and passed 60 checks across the two upstream revisions and their generated counterparts on macOS ARM64. Five expected answers changed after the source edit. [Companion review and evidence](https://github.com/Epiphytic/marmish-oddlang/pull/48)
