# Typed field relations

An `oddlang-field-relation-v1` graph defines portable field types and ordered comparisons on previously declared record types. Each type has a stable identity and a map from field IRIs to types. Values support booleans, unsigned integers, fixed bytes, byte vectors, text and references to declared types.

A relation names its receiver and an ordered list of typed arguments. In each step, a field operand selects the receiver or a record argument and one field. An argument operand selects one input. Comparison operands must have the same declared type. A relation without a `failure` returns a boolean.

A relation with a `failure` returns a typed result and stops at the first unequal comparison, returning its named mismatch error. A checked relation can also invoke a fallible method declared in `calls`. The receiver and ordered arguments must match the signature. A `check` step invokes a unit-returning method. A `result` operand compares the value of a fallible invocation. The generated binding propagates errors in step order and fixes the native error type in each call, without conversion. Procedures name argument IRIs when passing values to a call.

The Rust overlay supplies crate-local type paths, field names, native call names and method names. It binds each mismatch to a Rust error variant and optional static text argument. Record arguments are borrowed, opaque named arguments are passed by value, and byte and text arguments borrow slices. Byte and text fields and return values use owned vectors and strings. Equality borrows native operands. A comparison may directly use a checked return value without storing or subsequently exposing it.

The package component `field-relation` adds inherent methods to generated types. Its package entry has no reexports of its own. [Independent precedence fixture](../../verification/field-relation/verify.py)
