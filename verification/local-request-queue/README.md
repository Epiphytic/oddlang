# Durable local request queue

A native package can include `local-request-queue` with a pinned `oddlang-local-request-queue-v1` graph and a pinned Rust overlay selecting `sqlite`. The graph identifies the queue contract, sets fixed byte widths for the account, room, and both identifiers, and caps the encoded frame at one byte through one MiB. The package supplies `rusqlite` and `sha2`.

The authorized owner passes its open, file-backed connection and the exact authorized account and room to `LocalQueue::attach`. The queue uses the main database and requires a durable journal mode with at least full synchronization. A caller can key SQLCipher before attaching, and maintain its unrelated state in the same database.

The owner checks the application contract and supplies the already canonicalized bytes. The queue checks their size and SHA-256, and persists the original bytes in the same transaction as their identities. `enqueue` reports `Queued` after commit. An exact replay reports `AlreadyQueued` with the same receipt. A changed frame or identity under the same request ID is a request collision. A different initial request under the same invocation is an invocation collision. Follow-ups use separate request IDs and require an existing initial invocation. Reads require the same account and room and verify the stored frame digest.

```sh
cargo +1.97.1 build --offline --locked --bin oddlang
python3 -B verification/local-request-queue/check.py target/debug/oddlang
```

The river gauge fixture compiles with warnings denied and exercises commit, duplicate replay, scope and digest rejection, bounds, collisions, rollback, and exact recovery in a separate process against a keyed SQLCipher database with pre-existing owner state. It verifies that an unkeyed connection cannot read the state. The outcome records local queue acceptance.
