# Command surface

The `command-surface-v1` graph records parser behavior and help metadata in an ordered command tree. The Rust projection uses Clap 4.6.7 to interpret arguments and format help, version and diagnostics. Each executable supplies its own binding for the invocation name, invalid-argument status and diagnostic label, uncovered status and map from canonical command paths to operation IRIs.

Recover a candidate from one Rust file with a selected Clap `Parser` derive. The selection records the root type, Cargo package name, version and identified coverage gaps. Recovery follows a reachable `Subcommand` enum and tuple-variant `Args` carriers in that file. It recovers flags, scalar and repeated parameters, positions, defaults, aliases, hidden commands, grouping, conflicts, requirements and supported literal value restrictions. Unsupported derive settings fail with a diagnostic. Review the candidate against the actual Cargo profile and independent process observations. Rust name resolution outside the selected file, custom parse functions, multiple help paragraphs, per-value descriptions, nested derive subcommands and bootstrap logic need additional recovery.

```sh
oddlang recover cli-surface SOURCE.rs SELECTION.json GRAPH.json
oddlang build cli-surface rust GRAPH.json BINDING.json cli.rs
```

A binding key is the canonical command path with slash-separated components, an empty string for no selected command, or `*` for an external subcommand. The emitted `parse` reports a terminal Clap response, a matched command and its typed native bindings, or an uncovered command. `run_cli` writes parser responses to the provider-selected stream and returns the bound status. `run_cli_with` allows a native consumer to bind known operation IRIs to effects. A native package overlay can map an operation IRI to a checked crate-relative local command handler with `handlers`. All unimplemented operations return the uncovered status. External subcommand arguments are parser data and the projection does not launch a process.

Native package manifests can include the `cli-surface` component with pinned graph and overlay and select `run_cli` as a reexport. The manifest declares the exact Clap 4.6.7 dependency.

The independent [observatory fixture](../../verification/command-surface/) compares the Rust derive and generated implementations using captured process stdout, stderr and status. It exercises help, version, hidden commands, option and command aliases, required values, conflicts, requirements, numeric bounds, global defaults and callback dispatch. The generated build consumes graph and binding. Independent assertions pin exit status, stream routing, diagnostic suffix and a successful dispatch. Mutations change a numeric boundary and reject invalid defaults, positional order, provider pins and unsupported derive metadata.

```sh
cargo build --locked --offline
for suite in check prefix terminal; do
  cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin command-surface -- "$suite" target/debug/oddlang
done
```

A graph may declare `terminal_subparsers`, keyed by canonical immediate
subcommand name. Its primary command must be an unbound leaf without aliases. When that token immediately follows the executable, the
projection gives the remaining tokens to the independent, versionless parser
under its own display name. Its grammar has no inherited global arguments and
no child commands. Help and usage errors use the usual terminal response
binding. A successful auxiliary parse returns the uncovered outcome before
dispatch. The root command's generated `help NAME` keeps using the primary
grammar. Option-prefix routing and custom validation outside the declaration
require separate rules and evidence.

A recovery selection with an empty version expects a `Parser` derive without
`#[command(version)]`; its recovered root can supply the terminal subparser
grammar. Recovery preserves an explicit literal command name and supports the
one-or-more trailing positional shape and hyphen-valued arguments.

A terminal leaf can additionally declare `terminal_prefixes`. Each policy is
keyed by the canonical leaf name and supplies allowed global option IDs and
a finite denial diagnostic. The renderer reads the primary grammar's global
long and short spellings and single-token value arities. It recognizes
attached and separate option values. Built-in root help and version retain
their precedence.

A denied prefix returns its terminal diagnostic before auxiliary parsing.
A permitted prefix contributes a response only when the auxiliary parser
returns help or a usage error. Successful auxiliary parsing continues through
the existing unbound primary path. Unrecognized options, prefix clusters
and separate option values that look like flags stay with the primary parser.
A permitted prefix with a trailing separator does too. The profile does
not project arbitrary pre-parser behavior.

```sh
oddlang recover cli-terminal-prefix MODULE.rs SELECTION.json POLICY.json
```

The selection names the literal allowlist function, direct rejection branch,
raw error formatter and bound option identifiers. Recovery checks those
bounded source shapes. The lexical prefix scan and its stopping rules are
authored profile semantics, qualified against independently compiled native
observations. Rust name resolution outside the selected source requires its
own evidence. An unrelated two-revision fixture changes one allowed option
in source and observes the resulting help-versus-denial change.
