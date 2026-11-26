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
