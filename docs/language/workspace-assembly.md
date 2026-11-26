# Mixed-source workspace assembly, version 1

`oddlang assemble PROJECT.toml INPUT_STORE DESTINATION` publishes a Rust source tree from a pinned projection and retained files. The destination must be absent or an empty directory, with an existing parent. Its entries and executable modes come from the manifest. Assembly stages and validates the complete tree before publishing it by a directory rename.

```toml
version = 1
crates = ["application", "contract"]
target = "aarch64-apple-darwin"
features = ["application/codec"]
importSha256 = "<SHA-256 of exact import.lock bytes>"
graphSha256 = "<64 lowercase hexadecimal digits>"
projectionSha256 = "<SHA-256 of exact projection.json bytes>"
cargoLockSha256 = "<SHA-256 of root Cargo.lock>"

[[files]]
path = "Cargo.lock"
mode = "100644"
producer = "retained"
sha256 = "<SHA-256 of the lock>"

[[files]]
path = "contract/src/frame.rs"
module = "frame.rs"
mode = "100644"
producer = "generated"
sha256 = "<SHA-256 of the emitted module>"
```

Include one row for every workspace file, including all manifests, library roots, build inputs and static assets. Mode `100755` denotes an executable retained file. Enumerate the selected package names and explicit features as `package/feature`. The target and feature set describe the build configuration for which the producer pins apply. Cargo and rustc subsequently evaluate dependencies and configuration directives.

The store layout is closed. The imported graph, import lock and verified source map are supplied under `imports/IMPORT_SHA256/` as `domain.ttl`, `import.lock` and `source-map.json`. Retained bytes live at `retained/SHA256`. The publisher copies the verified Rust receipt and the exact files listed by it to `projections/PROJECTION_SHA256/`. It contains `projection.json` and the module files named by its map. The manifest has exactly three JSON fields.

```json
{"version":1,"graphSha256":"<digest>","files":{"frame.rs":"<digest>"}}
```

File paths use slash-separated portable ASCII segments. Each emitted Rust module maps to exactly one generated workspace destination. SHA-256 covers file bytes. The qualified build requires an independently authenticated project manifest. Its `importSha256` pins the exact cache release, alongside the pinned projection receipt, Cargo lock and graph digests. The assembler copies the three import artifacts into a fresh scratch directory and verifies the framing release before generating the module. It compares the emitter's receipt and module bytes directly against the candidate bytes before publication. The original source tree and the candidate projection are absent from the emitter inputs. The generated artifact then retains its locked destination mode and digest.

Literal Rust file includes and Cargo build-script paths must resolve within the staged tree. Retained build scripts and procedural macros are executable inputs. Runtime file reads by those programs require the separate isolated build and dependency inspection to establish a build-closure claim.

## Version 2

A version 2 workspace uses both the framing and Rust facade emitters. Replace the three top-level import, graph and projection digests with two emitter declarations. Keep the top-level Cargo lock digest, selected crates, target and features.

```toml
[[emitters]]
profile = "framing-v1"
package = "contract"
importSha256 = "<digest of framing import.lock>"
graphSha256 = "<digest of framing domain.ttl>"
projectionSha256 = "<digest of framing projection.json>"

[[emitters]]
profile = "rust-facade-v1"
package = "contract"
importSha256 = "<digest of facade import.lock>"
graphSha256 = "<digest of facade domain.ttl>"
projectionSha256 = "<digest of facade projection.json>"
```

Each generated file row adds an `emitter` field naming its profile. Each retained row uses the version 1 fields. Both profiles use the store layout described above. Each import, graph and projection digest must differ between profiles. The import locks identify the package and target. Each generated file must occupy the named Cargo package’s `src/<module>` path. These rules allow the two profiles to contribute modules to the same package or separate packages.

Assembly dispatches each pinned release to its named emitter in a fresh directory, checks the resulting receipt and compares the generated module bytes. The selected modules must consume both receipts in full. The project manifest supplies the independent release pins and must be authenticated by the caller.

### Process limits at executable startup

The `process-limit` native component accepts an `oddlang-process-limit-v1`
graph. Its `id` and `failure` are distinct absolute IRIs. Select
`resource: "open-descriptors"` and give `platforms.linux` and
`platforms.macos` a policy. `{"kind":"no-op"}` returns success without
calling libc. `{"kind":"lower-soft","ceiling":384}` requires an explicit
`libc` dependency and a positive unsigned 32-bit ceiling. It reads both
limits, sets the soft limit to the smaller of its current value and the
ceiling when needed, then verifies both the resulting soft limit and the
original hard limit. Existing descriptors and their flags are left alone.
Run this process-global operation during single-threaded startup before
other code mutates process limits.

The Rust overlay names the public `entry` and an `error` with `id`,
`type`, `variant`, `result`, and `display`. The stable error identity
must match the graph. The emitted function returns the declared result
alias with a unit success and the declared unit error on a syscall failure
or readback mismatch. The scope of the generated error is this operation.

An executable in a native workspace member may specify
`"startup":{"dependency":"device-limits","entry":["initialize"],"exit":1,"stderr":"device limits unavailable"}`.
The dependency must be a declared workspace edge and the named Rust
operation must type-check as a zero-argument function returning
`std::result::Result<(), E>`. On error, the wrapper writes the declared
stderr line and returns the nonzero exit code. On success it calls the
normally bound executable entry. The assembler checks both bindings with
Cargo before publishing. The generated wrapper hash is part of its package
lock.

### Public record views

`oddlang recover public-records LIB_RS SELECTION OUTPUT` matches selected
unconditional public module declarations and sibling Rust source files.
The selection format is `oddlang-rust-public-record-selection-v1`. It
pins `libSha256` and an ordered list of modules. Each module carries its
stable `id`, Rust `name`, source `sha256`, and optional `records` and
`aliases` maps from Rust names to stable IRIs. Recovery extracts the selected
named public fields in declaration order and accepts bool, u64, String and
Vec<u8>. It recognizes Clone and Serde Serialize derives and explicit
`pub use crate::module::Record` leaves, including grouped and renamed uses.
Selected conditional declarations, data-affecting attributes and direct
type shadows, inner non-doc attributes and unexpanded items are rejected.
A record module with a local glob import must bind the child's source
digest in `scopeGlobs`. The direct child must have an explicit sibling
`#[path]` declaration. Its visible prelude-name shadows and recursive
glob re-exports are rejected. The scope is a structural source slice.
Qualification compiles independent Rust callers to corroborate type and
trait resolution.

The output contains the graph, native name overlay, and recovery lock.
The graph records each ordered field's stable identity, serialized name
and admitted type, the declared traits, and the directed public alias
relations. The separate native overlay binds module, record and alias
identities to Rust identifiers. Recovery locks contain hashes of source
inputs and artifacts. They carry no Rust source text.

`oddlang build public-records rust GRAPH OVERLAY OUTPUT` or a
`public-records` native package component lowers the graph into public
module declarations. The renderer requires one definition per record.
It emits each alias as a Rust use of that same definition, so clients of
the two paths can exchange the value directly. A native package projecting
Serialize declares serde with the derive feature. Public module exports
are selected through the usual native package reexports.
