# Durable local request queue

Project a local request queue into a Rust package using the owner's existing SQLite connection. The owner supplies the authorized account and room and has already canonicalized the encoded request. The queue validates the scope, typed identities, frame bound and SHA-256 before storage. The owner sets up encryption and opens the connection before calling the queue.

- [ ] Project exact account, room and identity widths and an encoded frame byte limit into a pinned native component.
- [ ] Commit the initial invocation and its original request in one transaction. Accept an exact replay and preserve its original receipt. Reject a conflicting request or initial invocation. A follow-up has its own request identity and references an existing invocation.
- [ ] Read exact stored bytes after opening an independent connection and verify the digest. Exercise rejection and rollback with an unrelated native fixture. Record a successful commit only as local queue acceptance.
