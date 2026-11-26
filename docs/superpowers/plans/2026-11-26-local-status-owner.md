# Read-only local status owner

- [x] Model a versioned bounded request, an ordered status envelope, and legal lifecycle transitions. Keep the first serving phase tied to successful socket ownership.
- [x] Generate a native Unix listener with same-user credential checks, private path validation, bounded concurrency and frame time, ownership-aware cleanup, and shutdown on SIGINT or SIGTERM. Verify a separate application can publish a legal state transition.
- [ ] Compare the pinned original client against the generated owner on real sockets. Qualify the activation response and local shutdown, and leave provider readiness and original shutdown reports open.
