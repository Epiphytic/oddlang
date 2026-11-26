# Native file-backed credential preparation

The Linux and macOS projection uses a file-profile policy that names filesystem layout, private permission modes, CLI arguments, stable manifest field roles and finite diagnostics. The Rust binding selects the OS random provider and secp256k1 x-only public-key derivation.

```sh
oddlang build file-profile rust GRAPH.json RUST.json OUTPUT
```

The native callback selects explicit state and file backing, validates the profile name, acquires an exclusive lease, and opens bounded closed credential metadata. It creates a pending manifest if needed. It loads or creates the signing and storage key files and checks the stored signing public identity. It holds the lease through its completion report. Repeated preparation verifies existing credentials and preserves their files. New objects are published atomically without replacement, and manifest updates apply the ordered pending identity transitions. Runtime storage for credentials has private directories and exact regular-file ownership, link and mode checks.

The callback reports credential preparation. The generated profile remains pending until its required stores are initialized and checked by a completion provider. The preparation projection has a distinct entrypoint and effect catalog. The native package links that entry with checked path and string arguments.

```sh
python3 -B verification/file-profile/check.py target/debug/oddlang
```

An initialization extension declares a private runtime binding, named startup
diagnostics, a salt and distinct key derivation roles for two encrypted stores.
The native binding selects SQLCipher for the journal and a pinned account-device
provider for the account store. No remote transport starts during initialization.
Opening or binding either store, verifying the profile's stored identity, reading
declared counts and synchronizing the file and directory must succeed before the
pending record moves to ready.

The journal declares table and index layouts, column affinity, defaults,
primary and unique constraints, Boolean predicates, seed rows and filtered
metrics. An identity seed binds the store to the native signer's derived public
identity. JSON seed values declare ordered scalar properties. The compiler emits
parameterized inserts and constructs SQL expressions only from known operators
and declared identifiers. Current/pristine store reopening is differentiated
from application transitions that need replay or migrations, and the policy
identifies which relations must be empty for this target to reopen safely.

Standalone native packages can pin HTTPS Git dependencies to a full revision
and bind a registry replacement to a full revision. Cargo lock verification uses
the resulting manifest and the pinned lock, offline.

A configured extension declares prioritized OS environment sources, a bounded
private JSON file, dynamic relay and route fields, and an exact supported
profile of the remaining fields. Reopen validates the document before effects
and under the lease, and the native runner permits implicit backend selection
for an existing ready file-backed identity.

The guarded reopen profile admits recent durable records with valid timestamp
prefixes when the declared native recovery and journal relations are empty.
It reads the persisted replay floor, advances declared clocks monotonically,
reclassifies declared statuses, and refreshes validated static seed rows. A
configured journal can also add a versioned binding of domain-separated
SHA-256 public references derived from its profile and account identities.
These native operations share the encrypted lease and a transaction. The
projection computes status counts from the reopened store after those effects.
