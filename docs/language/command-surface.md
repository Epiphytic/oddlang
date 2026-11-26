# Command surface

The `command-surface-v1` graph records parser behavior and help metadata in an ordered command tree. The Rust projection uses Clap 4.6.7 to interpret arguments and format help, version and diagnostics. Each executable supplies its own binding for the invocation name, invalid-argument status and diagnostic label, uncovered status and map from canonical command paths to operation IRIs.

Recover a candidate from one Rust file with a selected Clap `Parser` derive. The selection records the root type, Cargo package name, version and identified coverage gaps. Recovery follows a reachable `Subcommand` enum and tuple-variant `Args` carriers in that file. It recovers flags, scalar and repeated parameters, positions, defaults, aliases, hidden commands, grouping, conflicts, requirements and supported literal value restrictions. Unsupported derive settings fail with a diagnostic. Review the candidate against the actual Cargo profile and independent process observations. Rust name resolution outside the selected file, custom parse functions, multiple help paragraphs, per-value descriptions, nested derive subcommands and bootstrap logic need additional recovery.

```sh
oddlang recover cli-surface SOURCE.rs SELECTION.json GRAPH.json
oddlang build cli-surface rust GRAPH.json BINDING.json cli.rs
```

A binding key is the canonical command path with slash-separated components, an empty string for no selected command, or `*` for an external subcommand. The emitted `parse` reports a terminal Clap response, a matched command and its typed native bindings, or an uncovered command. `run_cli` writes parser responses to the provider-selected stream and returns the bound status. `run_cli_with` allows a native consumer to bind known operation IRIs to effects. A native package overlay can map an operation IRI to a crate-relative Rust function with `handlers`. All unimplemented operations return the uncovered status. External subcommand arguments are parser data and the projection does not launch a process.

Native package manifests can include the `cli-surface` component with pinned graph and overlay and select `run_cli` as a reexport. The manifest declares the exact Clap 4.6.7 dependency.

The independent [observatory fixture](../../verification/command-surface/) compares the Rust derive and generated implementations using captured process stdout, stderr and status. It exercises help, version, hidden commands, option and command aliases, required values, conflicts, requirements, numeric bounds, global defaults and callback dispatch. The generated build consumes graph and binding. Independent assertions pin exit status, stream routing, diagnostic suffix and a successful dispatch. Mutations change a numeric boundary and reject invalid defaults, positional order, provider pins and unsupported derive metadata.

```sh
cargo build --locked --offline
python3 verification/command-surface/check.py target/debug/oddlang
```
