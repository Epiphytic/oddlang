# Generic JSON wire schema verification

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/wire-schema/verify.py target/debug/oddlang
```

The verifier recovers an unrelated irrigation event API from pinned Rust twice and compares all three outputs. It projects reviewed ordered guards with the recovered shape and compiles an unchanged Rust 2021 caller under warnings-as-errors. Controls cover malformed closed records and tags, bounded binary lists and hexadecimal values, changed source bytes, an unknown Serde option, an invalid guard field and switched guard precedence. The caller compares bytes from the original fixture and the generated declarations. [Schema profile](../../docs/language/json-wire-schema.md)

A nullable named field may declare `"requirePresence": true`. The member must appear in an input record. Both JSON null and a value of the declared item type are accepted. The generated Rust field is an `Option<T>` and always serializes the member, including null. A nullable field without this declaration continues to accept omission. The conformance catalog records the two input rules separately. The garden fixture compares omission, null, zero, 9007199254740993 and the largest u64.
