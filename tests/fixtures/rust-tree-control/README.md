# Independent Rust tree control

This edition-2024 package exercises native Rust regeneration on Apple Silicon
with Rust 1.97.1. It uses the pinned `bon` procedural macro, generic trait
dispatch from an integration test, and two configurations with different module
inputs. Cargo runs `build.rs` to produce `OUT_DIR/generated.rs` from the retained
asset `inputs/delta.txt`. The generated unit belongs in the compiler inventory
with the build script as its producer.

Use separate, empty Cargo targets when recording baseline and candidate
observations. The commands below run from this directory once the locked
dependency archives are in the local Cargo cache.

```sh
CARGO_TARGET_DIR="$(mktemp -d)" cargo +1.97.1 test --locked --offline
CARGO_TARGET_DIR="$(mktemp -d)" cargo +1.97.1 test --locked --offline --features extra
```

Each configuration runs two tests in `tests/observations.rs`. The default
result is 29 (15 from trait dispatch, 7 from the build asset, and 7 from
`src/basic.rs`). With `extra` enabled the result is 54 because
`src/extra.rs` contributes 32. The graph mutation changes the unique integer
leaf `7` in `src/basic.rs` to `8`. The default result must become 30 while
the unchanged independent test still expects 29. The `extra` result stays
54. This also tests that Cargo still resolves and compiles the correct branch.

The source controls include a nested block comment, a raw string containing
`"#`, adjacent and separated punctuation inside a declarative macro,
`file!()`, and a `line!()` which must continue to return 13. Check emitted
files byte for byte, including the intentional CRLF in `src/crlf.rs`, UTF-8
BOM in `src/bom.rs`, and the shebang in `src/bin/shebang.rs`. The tests
exercise the CRLF string normalization, an edition-2024 raw identifier,
punctuation match results and the shebang executable.

## Renamed control

Prepare the variant in a fresh copy before import. Change the package name
from `rust-tree-control` to `renamed-contract-probe` in `Cargo.toml`. Rename
the dependency key `roundtrip_proc` to `renamed_builder` in `Cargo.toml` and
the matching import in `src/lib.rs`. Keep the dependency package, version,
and the library target name. Regenerate the lockfile with
`cargo +1.97.1 generate-lockfile --offline`. Run both commands against this
copy before and after regeneration. The two independent tests and all
expected values are identical for the renamed package.
