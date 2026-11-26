# File-profile role and membership updates

A configured file-profile graph may declare a provisioning command. Its Rust overlay names a second entry point. The graph supplies the child argument names, role aliases, mutable membership sets, limits, key representation, diagnostics and ordered output columns. The CLI binding validates argument types on the specific subcommand. Setup uses the same profile lease, private key storage, manifest and configured-store validation as initialization.

The survey-registration unit test accepts a lower-hex panel membership declaration and rejects an unknown role, invalid report field, dangling role alias and colliding argument. The unrelated survey executable fixture exercises the original private credential operations and their rejection cases. Run both from the repository root.

```sh
cargo +1.97.1 test --locked --offline --lib
cargo +1.97.1 build --locked --offline --bin oddlang
python3 verification/file-profile/check.py target/debug/oddlang
```

On macOS ARM64 the library suite passed 17 tests and the unrelated executable fixture passed. The Marmish consumer separately compares generated and mainline setup over isolated temporary state. This page covers local profile operations only.

Membership sets declare the roles that can add entries. A role change preserves existing entries. An ordered report column with `active_role_only` counts members only in an allowed role. The downstream fixture changes a granted server to a client, attempts a forbidden grant, then changes back to a server. It compares complete reports and private configuration digests at every step for two relay spellings. The receiver must independently check role and authorization before dispatch. [Thirty-case receipt](https://github.com/Epiphytic/marmish-oddlang/blob/7fa56586df8e13253f3347a27ed157a0a3dfad31/verification/whole/basic-command-interop/local-profile-setup-aarch64-apple-darwin.json)
