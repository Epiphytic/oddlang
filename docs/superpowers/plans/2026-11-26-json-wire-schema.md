# JSON wire schema implementation plan

Recover reusable named record and adjacent-union shapes, project them as Rust types, and validate declared ordered guards with an unrelated caller. Keep semantic shape facts separate from Rust API bindings. Report unobserved semantics during structural recovery.

- [x] Define the wire graph and Rust binding. Verify deterministic projection of an unrelated closed record, nested optional and list values, and all three adjacent union payload forms.
- [x] Recover the same shape from pinned Rust through vetted Serde attributes. Record the source and selection digests and list unsupported attributes, references and operation behavior as gaps. Reject mutation of a pinned source.
- [x] Add typed unsigned bounds, presence conditions and ordered guards. Preserve the first declared error and show that changing the guard order changes the observed error.
- [ ] Recover and independently qualify canonical envelope admission and direction-sensitive operations. Extend guards with referenced operation calls, checked list aggregates and typed optional or variant traversal. Keep new-open clocks and referenced Copy Launch rules explicit until reviewed.

The first three checkpoints use an unrelated irrigation event schema. They exercise field, tag, shape, input and precedence mutations without depending on a product's source tree.
