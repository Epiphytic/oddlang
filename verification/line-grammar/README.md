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

Run `python3 -B verification/line-grammar/verify.py /path/to/oddlang` with
Rust 1.97.1. The unrelated fixture compiles the generated Rust and reads
its actual typed output. It also rejects malformed framed inputs, radix
spelling, a noncanonical version or decimal, duplicate captures, and
values outside their declared bounds.
