# Rust structural inventory, version 1

`oddlang recover rust Cargo.toml --profile PROFILE --out BUNDLE` invokes a
trusted compiler adapter for one Cargo target and publishes a candidate
inventory. Cargo build scripts and procedural macros execute during compiler
extraction. Run extraction on a trusted checkout with a trusted adapter.

The profile is a TOML table containing `adapter` (a path resolved relative to
the profile), `package`, `targetKind` (`lib` or `bin`), `targetName`,
`targetTriple`, `defaultFeatures`, optional `features`, `compilerCommit` and
`cargoLockSha256`. The adapter receives the absolute manifest path, package,
target kind, target name, target triple, feature mode (`default` or
`no-default`) and the sorted list of features. Optional `sourceRevision` verifies a clean Git checkout at the named revision. Optional `adapterInputs` lists relative paths contained in the adapter directory. Its output declares the crate
name and compiler commit. The command verifies those values and the workspace
Cargo.lock digest, and records these pins, the workspace and selected package manifest digests, and the entry point and declared adapter input digests. The lock records the discovered project, driver and Cargo home configuration file digests. Project Cargo configuration, macros and build scripts still participate. The adapter uses release Cargo mode.

`structure.ttl` uses the N-Triples subset of Turtle. Local subjects are
identified by package, crate and compiler definition path. Each segment uses
UTF-8 percent escapes outside ASCII alphanumerics, hyphen and underscore.
Definition paths for anonymous or generated items can include compiler
disambiguators. A different Cargo target or an edit to anonymous declarations
can change compiler paths. A rename across releases requires an explicit
identity mapping. Visibility reports compiler visibility at the definition.

The graph names declarations with `kind`, `name`, `owner`, `visibility`
and `macroGenerated`. Function signatures express inputs, output, ABI, safety and variadic status. Some type forms remain unknown and carry gaps. Edges name declaration dependencies and effect relationships. External targets use the namespace of the referring
inventory and carry a qualified `externalPath`. File, line and column
coordinates in adapter output are evidence only. Named `gap` facts describe
where the adapter could not account for a kind of node or relationship.
Bodies, syntax trees, ordered statements and raw signatures are outside
this vocabulary.

`structure.lock` contains the SHA-256 digest over the unique triples sorted
lexicographically by their UTF-8 bytes. Each line ends with space, period and
LF. There are no blank nodes. Adapter order and filesystem locations have no
effect on graph serialization. `oddlang recover validate-inventory INPUT
OUTPUT` validates and canonicalizes an adapter JSON file without a Cargo pin. Use `--package PACKAGE` after the output path to choose the same package scope as the recovery profile. The validator checks the permitted shape of facts. Independent compiler evidence establishes where those facts came from.
All bundles are labeled candidate. An independent, scoped reconciliation
must account for every declaration and edge and resolve every gap before
claiming exactness. A candidate structure makes no behavior compatibility claim.
