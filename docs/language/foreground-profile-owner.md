# Foreground profile owner

The `oddlang-foreground-profile-owner-v1` component links a generated command surface to a generated local status owner. It attaches to an existing, ready file-backed profile under a compatible nonblocking exclusive lease. The graph supplies argument identities, document field names and bounds, local path components, native activation policy and finite diagnostics. The Rust binding identifies the callback and the local status owner's exported `BoundOwner` type. The package linker validates that path and the shape and cardinality of every CLI argument the callback reads. Accepted CLI backend names map explicitly to the selected persisted provider.

Startup validates that the state and profile directories belong to the effective user and have mode 0700. Opened files must be single-link owner-only regular files. The foreground owner reads the bounded manifest and private credentials under the lease. It derives an x-only secp256k1 public key from the stored signing scalar and verifies it against the manifest. It also validates the second private file credential's size. It leaves the manifest and credentials in place. The basic profile checks credentials and owns the runtime socket.

The owner creates a new private runtime directory and versioned binding before publishing the status socket. The owner recovers a dead socket after a crash only under the matching lease, from a private runtime with the exact binding and no extra entries. It refuses live endpoints and unknown or conflicting directories. The binding points to the canonical profile path and commits fully before the socket becomes discoverable. Cleanup removes only the runtime entries this startup created, checks identity while holding the file descriptors, and holds the profile lease until cleanup finishes.

The `loopback_web_pki_tls` activation provider accepts one numeric-loopback secure relay and an explicit direct route mode. It makes a real TCP connection and a Rustls handshake against the pinned WebPKI roots under one bounded deadline. The held handshake may coexist with the graph's activating snapshot. Trust, I/O and deadline failures close the local owner. An authenticated TLS connection reaches an explicit provider-unavailable outcome because this profile ends at TLS, before WebSocket handoff and transport readiness. Neither transport success nor a socket binding independently authorizes a provider-ready state.

An optional `lifecycle` declaration adds the prepared-account provider lifetime. It names the storage profile IRI and the native overlay's `retained_stores` path must bind a generated file profile that exports `open_retained_stores`. The lifecycle owns the retained encrypted journal and account database, the account device, the relay SDK client and its notification forwarder. It declares the excluded profile entries and the explicit absent-engine role. Terminal field names and version come from the graph. A failed read-only preflight preserves the owned binding and returns before publishing a listener.

After socket service ends, the lifecycle observes actual account deactivation and relay termination. It waits for the notification forwarder to join its worker, closes both encrypted stores, and removes the runtime only when the provider closure succeeds. The executor is destroyed while the file lease is still held. A timed-out stage cannot yield a successful provider report. The [unrelated telemetry fixture](../../verification/foreground-profile-owner/stages.py) tests that an earlier failure still runs later stages and that elapsed deadlines do not claim completion.

The native package dependency schema can specify `defaultFeatures: false` to disable a provider's defaults before selecting specific Cargo features. The existing dependency defaults remain implicit when the field is absent.

The [field station fixture](../../verification/foreground-profile-owner/) uses unrelated graph and document names and a real loopback TLS listener. It covers private file security, credential identity, compatible lock contention, actual ClientHello, status discovery, signal cleanup, handshake deadline, abrupt process death and recovery, and live pathname preservation. It rejects an unresolved status owner binding.

```sh
cargo build --locked --offline
python3 verification/foreground-profile-owner/check.py target/debug/oddlang
```
