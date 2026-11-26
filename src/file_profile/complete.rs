use super::{Binding, Diagnostic, Policy, argument, component, distinct, schema::Journal};
use crate::local_command_client::RequiredArgument;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StorageProvider {
    SqlcipherAccountDevice,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Initialization {
    pub(super) relay_argument: String,
    pub(super) route_argument: String,
    local_route: String,
    pub(super) route_choices: Vec<String>,
    guards: Vec<Guard>,
    unsupported_peers_argument: String,
    retention_argument: String,
    retention_default: u64,
    runtime_suffix_allocation: u8,
    relay_scheme: String,
    runtime_prefix: String,
    socket_leaf: String,
    runtime_bound: u16,
    binding_leaf: String,
    binding_fields: [String; 3],
    journal_leaf: String,
    account_leaf: String,
    existing_state_exclusions: Vec<String>,
    account_version: u32,
    storage_salt: String,
    journal_role: String,
    account_role: String,
    identity_key: String,
    configuration: Diagnostic,
    invalid_route: Diagnostic,
    invalid_relay: Diagnostic,
    runtime_path: Diagnostic,
    runtime_collision: Diagnostic,
    journal_error: Diagnostic,
    account_error: Diagnostic,
    preflight_error: Diagnostic,
    reload_unsupported: Diagnostic,
    journal: Journal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) configured: Option<super::configured::Configured>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Guard {
    False {
        argument: String,
        rejected: Diagnostic,
    },
    Absent {
        argument: String,
        rejected: Diagnostic,
    },
}

impl Guard {
    fn input(&self) -> (&str, RequiredArgument, &Diagnostic) {
        match self {
            Self::False { argument, rejected } => (argument, RequiredArgument::Flag, rejected),
            Self::Absent { argument, rejected } => {
                (argument, RequiredArgument::OptionalValue, rejected)
            }
        }
    }
}

impl Initialization {
    pub(super) fn has_config(&self) -> bool {
        self.configured.is_some()
    }
    pub(super) fn validate(&self, base: &Policy) -> Result<(), String> {
        let layout = &base.layout;
        if !distinct(
            [
                &base.arguments.state,
                &base.arguments.profile,
                &base.arguments.backend,
                &self.relay_argument,
                &self.route_argument,
                &self.unsupported_peers_argument,
                &self.retention_argument,
            ]
            .into_iter()
            .map(String::as_str)
            .chain(self.guards.iter().map(|guard| guard.input().0)),
        ) || ![
            &self.relay_argument,
            &self.route_argument,
            &self.unsupported_peers_argument,
            &self.retention_argument,
        ]
        .into_iter()
        .all(|v| argument(v))
            || !distinct(self.route_choices.iter().map(String::as_str))
            || !self.route_choices.contains(&self.local_route)
            || self.runtime_suffix_allocation >= self.runtime_bound as u8
            || self.guards.iter().any(|guard| {
                let (id, _, error) = guard.input();
                !argument(id)
                    || error.exit == 0
                    || error.stderr.is_empty()
                    || error.stderr.chars().any(char::is_control)
            })
            || !distinct(
                [
                    &layout.owner,
                    &layout.keys,
                    &layout.manifest,
                    &self.journal_leaf,
                    &self.account_leaf,
                ]
                .into_iter()
                .chain(layout.private_subdirectories.iter())
                .map(String::as_str),
            )
            || ![
                &self.runtime_prefix,
                &self.socket_leaf,
                &self.binding_leaf,
                &self.journal_leaf,
                &self.account_leaf,
            ]
            .into_iter()
            .all(|v| component(v))
            || !distinct(self.existing_state_exclusions.iter().map(String::as_str))
            || !self
                .existing_state_exclusions
                .iter()
                .all(|path| component(path))
            || self.binding_leaf == self.socket_leaf
            || !distinct(self.binding_fields.iter().map(String::as_str))
            || !distinct([self.journal_role.as_str(), self.account_role.as_str()])
            || ![
                &self.local_route,
                &self.relay_scheme,
                &self.storage_salt,
                &self.identity_key,
            ]
            .into_iter()
            .all(|v| !v.is_empty() && !v.chars().any(char::is_control))
            || !(32..=255).contains(&self.runtime_bound)
            || self.diagnostics().iter().any(|(_, d)| {
                d.exit == 0 || d.stderr.is_empty() || d.stderr.chars().any(char::is_control)
            })
        {
            return Err("invalid encrypted profile initialization policy".into());
        }
        Ok(())
    }

    pub(super) fn callback_arguments(self) -> Vec<(String, RequiredArgument)> {
        let mut arguments = vec![
            (self.relay_argument, RequiredArgument::RepeatedString),
            (self.route_argument, RequiredArgument::String),
            (
                self.unsupported_peers_argument,
                RequiredArgument::RepeatedString,
            ),
            (self.retention_argument, RequiredArgument::Unsigned64),
        ];
        arguments.extend(self.guards.iter().map(|guard| {
            let (arg, kind, _) = guard.input();
            (arg.to_owned(), kind)
        }));
        arguments
    }

    fn diagnostics(&self) -> [(&'static str, &Diagnostic); 9] {
        [
            ("INVALID_ROUTE", &self.invalid_route),
            ("CONFIGURATION", &self.configuration),
            ("INVALID_RELAY", &self.invalid_relay),
            ("RUNTIME_PATH", &self.runtime_path),
            ("RUNTIME_COLLISION", &self.runtime_collision),
            ("JOURNAL", &self.journal_error),
            ("ACCOUNT", &self.account_error),
            ("PREFLIGHT", &self.preflight_error),
            ("RELOAD_UNSUPPORTED", &self.reload_unsupported),
        ]
    }

    pub(super) fn argument_guards(&self) -> String {
        let mut guards = String::new();
        for guard in &self.guards {
            let (id, kind, rejected) = guard.input();
            let expression = match kind {
                RequiredArgument::Flag => format!(
                    "*matches.try_get_one::<bool>({id:?}).map_err(|_| BINDING)?.unwrap_or(&false)"
                ),
                _ => format!("matches.contains_id({id:?})"),
            };
            writeln!(
                guards,
                "        if {expression} {{ return Err(Failure({}, {:?})); }}",
                rejected.exit, rejected.stderr
            )
            .unwrap();
        }
        guards
    }

    pub(super) fn render(
        &self,
        out: &mut String,
        policy: &Policy,
        binding: &Binding,
    ) -> Result<(), String> {
        out.push_str(include_str!("complete.rs.in"));
        out.push_str(&self.journal.render(&self.identity_key)?);
        if let Some(configured) = &self.configured {
            configured.render(out, policy.provisioning.is_some())?;
            configured.storage.render(&self.journal, out)?;
        } else {
            out.push_str(include_str!("explicit.rs.in"));
            writeln!(out, "const ROUTE: &str = {:?};", self.local_route).unwrap();
        }
        for (name, diagnostic) in self.diagnostics() {
            writeln!(
                out,
                "const {name}: Failure = Failure({}, {:?});",
                diagnostic.exit, diagnostic.stderr
            )
            .unwrap();
        }
        for (name, value) in [
            ("SCHEME", &self.relay_scheme),
            ("RUNTIME_PREFIX", &self.runtime_prefix),
            ("SOCKET_LEAF", &self.socket_leaf),
            ("BINDING_LEAF", &self.binding_leaf),
            ("JOURNAL_LEAF", &self.journal_leaf),
            ("ACCOUNT_LEAF", &self.account_leaf),
            ("STORAGE_SALT", &self.storage_salt),
            ("JOURNAL_ROLE", &self.journal_role),
            ("ACCOUNT_ROLE", &self.account_role),
            ("IDENTITY_KEY", &self.identity_key),
        ] {
            writeln!(out, "const {name}: &str = {value:?};").unwrap();
        }
        writeln!(out, "const ROUTE_CHOICES: &[&str] = &{:?};\nconst RUNTIME_SUFFIX_ALLOCATION: usize = {};\nconst RUNTIME_BOUND: usize = {};\n#[derive(serde::Serialize, serde::Deserialize, PartialEq)]\n#[serde(deny_unknown_fields)]\nstruct RuntimeBinding {{",
            self.route_choices, self.runtime_suffix_allocation, self.runtime_bound).unwrap();
        for (wire, field, ty) in [
            (&self.binding_fields[0], "version", "u8"),
            (&self.binding_fields[1], "identity", "String"),
            (&self.binding_fields[2], "root", "std::path::PathBuf"),
        ] {
            writeln!(out, "    #[serde(rename = {wire:?})]\n    {field}: {ty},").unwrap();
        }
        writeln!(out, "}}\nconst EXCLUDED_PROFILE_ENTRIES: &[&str] = &{:?};\nconst ACCOUNT_VERSION: u32 = {};",
            self.existing_state_exclusions, self.account_version).unwrap();
        let args = &policy.arguments;
        let guards = self.argument_guards();
        let (select, before_credentials, effects, backend_guard, native_check) = if let Some(
            configured,
        ) =
            &self.configured
        {
            if [
                &policy.layout.owner,
                &policy.layout.keys,
                &policy.layout.manifest,
                &self.account_leaf,
                &self.journal_leaf,
            ]
            .into_iter()
            .chain(policy.layout.private_subdirectories.iter())
            .any(|leaf| configured.leaf == *leaf)
            {
                return Err("persisted configuration aliases another profile file".into());
            }
            (
                format!(
                    r#"
        let base = selected_state(matches.try_get_one::<std::path::PathBuf>({:?}).map_err(|_| BINDING)?)?;
        let label = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str).unwrap_or({:?});
        if !atom(label) {{ return Err(INVALID_PROFILE); }}
        let explicit = matches.try_get_many::<String>({:?}).map_err(|_| BINDING)?
            .map(|values| values.cloned().collect::<Vec<_>>());
        let selected = if let Some(relays) = &explicit {{
            let route = route.unwrap_or(DEFAULT_ROUTE);
            (route.to_owned(), configured_relays(route, relays)?)
        }} else {{
            check_directory(&base.join(label))?;
            selected_config(&base.join(label))?.ok_or(IO)?
        }};
        let bound = std::cell::Cell::new(false);
        let base = &base;"#,
                    args.state, args.profile, args.default_profile, self.relay_argument
                ),
                r#"|root, record| {
                if default_storage && record.phase != READY { return Err(BACKEND_REQUIRED); }
                runtime_directory(record)?;
                let stored = selected_config(root)?;
                if stored.as_ref().is_some_and(|config| config != &selected) { return Err(CONFIG_CONFLICT); }
                if explicit.is_none() && stored.is_none() { return Err(IO); }
                bound.set(stored.is_some());
                Ok(())
            }"#,
                "|database, snapshot| configured_effects(database, snapshot, bound.get())",
                "let default_storage = backend == Some(AUTO_BACKEND);\n        if !default_storage && backend != Some(file_argument) { return Err(BACKEND_REQUIRED); }",
                "configured_native_ready",
            )
        } else {
            (
                format!(
                    r#"
        let relays = matches.try_get_many::<String>({:?}).map_err(|_| BINDING)?.ok_or(CONFIGURATION)?;
        check_relays(route, relays)?;
        let base = matches.try_get_one::<std::path::PathBuf>({:?}).map_err(|_| BINDING)?.ok_or(STATE_REQUIRED)?;
        let label = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str).unwrap_or({:?});"#,
                    self.relay_argument, args.state, args.profile, args.default_profile
                ),
                "|_, record| runtime_directory(record).map(|_| ())",
                "|_, _| Ok(())",
                "if backend != Some(file_argument) { return Err(BACKEND_REQUIRED); }",
                "|_, _| Ok(())",
            )
        };
        writeln!(out, "pub fn {}(matches: clap::ArgMatches) -> std::process::ExitCode {{
    let run = || -> Result<(), Failure> {{
{guards}
        let route = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str);
        if route.is_some_and(|route| !ROUTE_CHOICES.contains(&route)) {{ return Err(INVALID_ROUTE); }}
        if matches.try_get_many::<String>({:?}).map_err(|_| BINDING)?.is_some()
            || matches.try_get_one::<u64>({:?}).map_err(|_| BINDING)?.is_some_and(|value| *value != {}) {{
            return Err(CONFIGURATION);
        }}
{select}
        let backend = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str);
        let file_argument = {:?};
{backend_guard}
        let mut metrics = Vec::new();
        let profile = prepare(base, label, {before_credentials},
            |root, record, signing, storage| {{
                metrics = complete(root, record, signing, storage, {native_check}, {effects})?;
                Ok(())
            }})?;
        print_initialized(&profile, &metrics)
    }};
    match run() {{
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => error.report(),
    }}
}}", binding.entry, self.route_argument, self.unsupported_peers_argument, self.retention_argument, self.retention_default, args.backend, args.file_backend).unwrap();
        Ok(())
    }
}
