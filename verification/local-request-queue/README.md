# Durable local request queue

A native package can include `local-request-queue` with a pinned `oddlang-local-request-queue-v1` graph and a pinned Rust overlay selecting `sqlite`. The graph identifies the queue contract, sets fixed byte widths for the account, room, and both identifiers, and caps the encoded frame at one byte through one MiB. The tested target pins `rusqlite` 0.40.1 and `sha2` 0.10.9.

The authorized owner passes its open, file-backed connection and the exact authorized account and room to `LocalQueue::attach`. The queue uses the main database and requires a durable journal mode with at least full synchronization. The owner controls encryption, database opening, and filesystem permissions. To use SQLCipher, the owner must key the database before attaching.

The owner checks the application contract and supplies the already canonicalized bytes. The queue checks their size and SHA-256, and persists the original bytes in the same transaction as their identities. `enqueue` reports `Queued` after commit. An exact replay reports `AlreadyQueued` with the same receipt. A changed frame or identity under the same request ID is a request collision. A different initial request under the same invocation is an invocation collision. Follow-ups use separate request IDs and require an existing initial invocation. Reads by request ID require the same account and room and verify the stored frame digest. After a storage or busy error the owner can retry the identical request to resolve whether a prior transaction committed.

```sh
cargo +1.97.1 build --offline --locked --bin oddlang
python3 -B verification/local-request-queue/check.py target/debug/oddlang
```

The river gauge fixture compiles with warnings denied and exercises commit, duplicate replay, scope and digest rejection, bounds, collisions, rollback, and exact recovery in a separate process against a keyed SQLCipher database with WAL, full synchronization, and pre-existing owner state. It also checks stored digest corruption, malformed queue schema, a rejected commit caused by a deferred constraint, and a concurrent replay from two processes. It verifies that an unkeyed connection cannot read the state. The outcome records local queue acceptance and recovery across process restart. Power-loss persistence depends on the owner’s SQLite configuration, filesystem, and storage device.

The fixture and all 18 compiler library tests passed on macOS ARM64 with Rust 1.97.1. The ontology parsed as Turtle and the ready-work query completed.
