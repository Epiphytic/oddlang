# Bounded private buffered input

Compiler shape recovery selects one named record with one type parameter.
The record has four fields. The input stores that parameter directly.
Storage wraps a positive fixed octet array in registry zeroize's
`Zeroizing`. The two cursors are unsigned pointer-sized integers of the
same width. Recovery preserves field order and array extent, records the
observation and selection hashes, and rejects incomplete selected shapes.
The observation supplies structural facts, while the selection supplies
the stable identity, field roles and transition policies. Native behavior
and privacy require separate qualification.

The native Rust projection implements `Read` and `BufRead` for the private
record. When the cursors meet, refill resets the begin cursor and calls
the input's `Read`. A successful call sets the end to the number of bytes
read, including zero on EOF. A failed call returns that error and either
retains the previous end or discards the window, as the graph declares.
The retained extent refers to the actual backing bytes. Erased bytes stay
zero when a failing input leaves storage untouched. An empty read either
attempts the same refill or returns zero immediately. Consumption clamps
to the current window and either erases or retains the consumed bytes.
Storage erases on drop through locked zeroize 1.9.1.

The contract assumes a conforming Rust `Read` provider, valid initialized
cursors and a buffer layout accepted by the target. Projection emits no
construction API, public exports, provider bridge or additional effects.
