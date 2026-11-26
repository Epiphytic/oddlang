# Native file-backed credential preparation

A file-profile policy names filesystem layout, private permission modes, CLI arguments, stable manifest field roles and finite diagnostics. The Rust binding selects the OS random provider and secp256k1 x-only public-key derivation.

```sh
oddlang build file-profile rust GRAPH.json RUST.json OUTPUT
```

The native callback selects explicit state and file backing, validates the profile name, acquires an exclusive lease, and opens bounded closed credential metadata. It creates a pending manifest if needed. It loads or creates the signing and storage key files and checks the stored signing public identity. It holds the lease through its completion report. Repeated preparation verifies existing credentials and preserves their files. New objects are published atomically without replacement, and manifest updates apply the ordered pending identity transitions. Runtime storage for credentials has private directories and exact regular-file ownership, link and mode checks.

The callback reports credential preparation. The generated profile remains pending until its required stores are initialized and checked by a completion provider. The preparation projection has a distinct entrypoint and effect catalog. The native package links that entry with checked path and string arguments.

```sh
python3 -B verification/file-profile/check.py target/debug/oddlang
```
