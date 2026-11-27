# Ordered line grammar qualification

`oddlang build line-grammar rust GRAPH OVERLAY OUTPUT` generates a native
Rust parser from the `oddlang-line-grammar-v1` graph. A graph declares a
positive byte ceiling, forbidden ASCII bytes, and a nonempty sequence of
lines. The grammar requires ASCII input, a terminal LF, exactly one ASCII
space between nonempty tokens, and an exact row and token count. A nested
split token splits at the first declared delimiter and applies a complete
rule to each part. Nested text grammars are the existing
`oddlang-text-grammar-v1` declarations, addressed by their stable identity.

Tokens declare exact literals, text with optional choices or subgrammar,
canonical unsigned decimal, lowercase hexadecimal, two literal boolean
spellings, a literal mapped to an unsigned value, or delimiter splitting.
The Rust capture overlay maps each capture to a distinct field of a named
native record. Borrowed text carries the input lifetime. Literal captures
are static strings. Decoded hexadecimal captures are fixed byte arrays.
Each scalar integer has the declared native unsigned width.

Run `cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin line-grammar -- inventory /path/to/oddlang` with
Rust 1.97.1. The unrelated fixture compiles the generated Rust and reads
its actual typed output. It also rejects malformed framed inputs, radix
spelling, a noncanonical version or decimal, duplicate captures, and
values outside their declared bounds.

The record overlay `oddlang-line-record-rust-v1` maps declared output
records to native types and fields. It has a parser entry name, crate or
public visibility, a stable error ID and a native error variant with a
static text payload. Native package assembly checks that the referenced
error declaration is an existing matching export. Its `line-grammar`
component accepts the same pinned graph and overlay as the standalone
route.

A graph may validate finite tuples of captured scalars or checked sums
of captured unsigned integers at explicit one-based row positions. The
result layer constructs owned or static text, booleans, unsigned fields,
fixed arrays, and vectors of declared records from named captures. It
also binds simple formatters made from literal text, selected record text
fields and an optional textual argument.

Run `cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin line-grammar -- records /path/to/oddlang`
to compile and exercise an unrelated result with exact error and record
types, distinct finite-table and overflow failures, and checked package
error linkage.

Run the same command with the `support` subcommand to compile the crate-visible grammar predicate and ordered choice array and exercise their three binding refusals.
