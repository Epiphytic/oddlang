# Bounded local owner prepare

The local owner admits a same-user, framed prepare request on the leased client profile. Its normal status request remains available on the same private socket.

- [x] Generate opt-in dispatch in the local socket owner. Exercise it with the river gauge fixture and preserve its existing status, frame, socket ownership and concurrency bounds.
- [x] Project prepare request and reply fields, authorization checks, the pinned relay policy and the finite backend outcomes. Keep the application-specific names and profile rules in companion inputs.
- [x] Prepare the authorized fixture room with a durable local account, remote account, session and relay binding. Require acknowledged Welcome/control delivery for a ready reply. Fail closed on interrupted binding or revoked authorization.
- [ ] Verify duplicate prepare behavior, role, trust and relay rejection, termination and cleanup on a test-owned relay. Verify peer admission separately, without submitting a command.

The [warm native preview](https://github.com/Epiphytic/marmish-oddlang/blob/35efcb22cc38869f6b8be8863ec104dd2d3c1e97/verification/whole/basic-command-interop/bounded-owner-prepare-warm-preview-aarch64-apple-darwin.json) establishes the matching room, unavailable-discovery retry, authorization refusals, shutdown and cold reopen. The source-denied rebuild and the fixed client’s room replay subsequently passed. That replay checked wrong-peer and wrong-relay refusals, matching admission at the mainline server, repeated prepare, restart recovery, and cleanup. The receipt includes one mainline server start. An explicit wrong-role refusal is still unverified by this replay. [Fixed client build and room replay](https://github.com/Epiphytic/marmish-oddlang/blob/8c410e5536cccd6b11797b28393a3d1683088ede/verification/whole/basic-command-interop/PREPARE-ROOM.md)
