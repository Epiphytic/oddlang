# Foreground profile owner

- [x] Compose a generated CLI daemon entrypoint with the private local owner. Declare the supported foreground arguments and the persistent/runtime path binding.
- [x] Acquire a compatible exclusive lock on an existing ready private profile. Verify the declared file credentials, then create and publish a private owner runtime binding. Remove only owned runtime entries during teardown.
- [ ] Qualify pinned original-client discovery against generated `marmishd` and independently test lease contention, failure-before-publication and reattach. Preserve the parser baseline.
- [x] Bind a real bounded WebPKI TLS activation attempt. Keep fresh profile provisioning, provider databases, WebSocket/MDK readiness and installed lifetime open.
