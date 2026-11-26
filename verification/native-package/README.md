
### Native process targets and member workspaces

A native package can declare `binaries` as an array of objects with a Cargo binary name and an `entry` array of public native path segments. The generated process entry calls that zero-argument function and returns its `std::process::ExitCode`. Oddlang runs Cargo's offline locked binary check before publishing. The contract author supplies native path identifiers, never a Rust expression.

`oddlang build native-workspace rust WORKSPACE.json NEW_DIRECTORY` composes separately pinned package manifests. A version-one workspace specifies `format: oddlang-native-workspace-v1`, a root Cargo lock pin, and a `members` object keyed by native package names. Each member has a `manifest` file pin and optional `dependencies` listing other members by name. File pins contain `path` and `sha256`; relative paths resolve from the containing workspace document, and each member resolves its component inputs from its own package document. All output is staged and published together after Cargo validates the root resolution. Executable entries are type-checked after the local edges have been installed.

The root Cargo.lock governs combined dependency resolution and Cargo feature unification. The workspace receipt binds each standalone package receipt and the resulting member manifest. Compare observable behavior on this workspace configuration when adopting it. Run the two-package fixture with `python3 verification/native-workspace/verify.py /path/to/oddlang`.
