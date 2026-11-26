//! Transactional edits to role and peer membership in a private profile.
use super::{Arguments, Diagnostic, RequiredArgument, argument, distinct};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Provisioning {
    pub id: String,
    pub command: String,
    role_argument: String,
    role_field: String,
    update_argument: String,
    interactive_argument: String,
    default_role: String,
    default_relays: Vec<String>,
    onion_routes: Vec<String>,
    roles: BTreeMap<String, String>,
    sets: Vec<Membership>,
    report: Vec<Column>,
    max_peers: usize,
    peer_format: PeerFormat,
    invalid_member: Diagnostic,
    choice_required: Diagnostic,
    explicit_removal: Diagnostic,
    existing_config: Diagnostic,
    unsupported: Diagnostic,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PeerFormat {
    Nip19PublicKeyToLowerHex,
    LowerHex32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Membership {
    field: String,
    add: String,
    remove: String,
    clear_choice: Option<String>,
    roles: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Column {
    name: String,
    source: Source,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    Literal { value: serde_json::Value },
    Profile,
    PublicIdentifier,
    Backend,
    Role,
    AnyMember { field: String },
    Changed,
    Created,
    Opened,
    CleanupComplete,
}

impl Provisioning {
    pub(super) fn local_arguments(&self) -> Vec<(String, RequiredArgument)> {
        let mut args = vec![
            (self.role_argument.clone(), RequiredArgument::String),
            (self.update_argument.clone(), RequiredArgument::Flag),
            (self.interactive_argument.clone(), RequiredArgument::Flag),
        ];
        for set in &self.sets {
            args.push((set.add.clone(), RequiredArgument::RepeatedString));
            args.push((set.remove.clone(), RequiredArgument::RepeatedString));
            if let Some(choice) = &set.clear_choice {
                args.push((choice.clone(), RequiredArgument::Flag));
            }
        }
        args
    }
    pub(super) fn validate(
        &self,
        entry: &str,
        previous: &str,
        fixed: &serde_json::Map<String, serde_json::Value>,
        routes: &[String],
    ) -> Result<(), String> {
        let args = self.local_arguments();
        if !self.id.starts_with("urn:")
            || !argument(&self.command)
            || entry == previous
            || entry.starts_with("provision_")
            || syn::parse_str::<syn::Ident>(entry).is_err()
        {
            return Err("invalid provisioning operation or native entry".into());
        }
        if !distinct(args.iter().map(|(name, _)| name.as_str()))
            || args.iter().any(|(name, _)| !argument(name))
        {
            return Err("provisioning requires distinct command arguments".into());
        }
        if self.default_relays.is_empty()
            || self.default_relays.iter().any(|s| s.is_empty())
            || !distinct(self.onion_routes.iter().map(String::as_str))
            || self.onion_routes.iter().any(|r| !routes.contains(r))
        {
            return Err("invalid provisioning relay defaults or onion route choices".into());
        }
        let known = |role: &str| self.roles.get(role).is_some_and(|value| value == role);
        if !known(&self.default_role)
            || self.roles.iter().any(|(k, v)| k.is_empty() || !known(v))
            || !fixed
                .get(&self.role_field)
                .and_then(|v| v.as_str())
                .is_some_and(known)
        {
            return Err("invalid provisioning role declarations".into());
        }
        if self.sets.is_empty()
            || !(1..=65536).contains(&self.max_peers)
            || !distinct(
                self.sets
                    .iter()
                    .map(|s| s.field.as_str())
                    .chain([self.role_field.as_str()]),
            )
        {
            return Err("invalid provisioning membership fields".into());
        }
        for set in &self.sets {
            if set.roles.is_empty()
                || !distinct(set.roles.iter().map(String::as_str))
                || !set.roles.iter().all(|r| known(r))
                || !fixed
                    .get(&set.field)
                    .and_then(|v| v.as_array())
                    .is_some_and(Vec::is_empty)
            {
                return Err(format!(
                    "membership {} requires known roles and an empty initial set",
                    set.field
                ));
            }
        }
        if self.report.is_empty() || !distinct(self.report.iter().map(|c|c.name.as_str()))
            || self.report.iter().any(|c| matches!(&c.source,Source::AnyMember{field} if !self.sets.iter().any(|s|&s.field==field))) {
            return Err("invalid provisioning report column".into());
        }
        for diagnostic in [
            &self.invalid_member,
            &self.choice_required,
            &self.explicit_removal,
            &self.existing_config,
            &self.unsupported,
        ] {
            if diagnostic.exit == 0
                || diagnostic.stderr.is_empty()
                || diagnostic.stderr.chars().any(char::is_control)
            {
                return Err("invalid provisioning diagnostic".into());
            }
        }
        Ok(())
    }
    pub(super) fn render(
        &self,
        out: &mut String,
        args: &Arguments,
        init: &super::complete::Initialization,
        entry: &str,
    ) -> Result<(), String> {
        let encode_text = matches!(self.peer_format, PeerFormat::Nip19PublicKeyToLowerHex);
        writeln!(out, "const PROVISION_NIP19: bool = {encode_text};").unwrap();
        for (key, d) in [
            ("MEMBER", &self.invalid_member),
            ("CHOICE", &self.choice_required),
            ("REMOVE", &self.explicit_removal),
            ("EXISTS", &self.existing_config),
            ("SCOPE", &self.unsupported),
        ] {
            writeln!(
                out,
                "const PROVISION_{key}: Failure = Failure({}, {:?});",
                d.exit, d.stderr
            )
            .unwrap();
        }
        for (key, v) in [
            ("COMMAND", &self.command),
            ("ROLE_ARG", &self.role_argument),
            ("ROLE_FIELD", &self.role_field),
            ("UPDATE", &self.update_argument),
            ("INTERACTIVE", &self.interactive_argument),
            ("DEFAULT_ROLE", &self.default_role),
            ("STATE", &args.state),
            ("PROFILE", &args.profile),
            ("BACKEND_ARG", &args.backend),
            ("FILE", &args.file_backend),
            ("DEFAULT_PROFILE", &args.default_profile),
            ("ROUTE", &init.route_argument),
            ("RELAYS", &init.relay_argument),
        ] {
            writeln!(out, "const PROVISION_{key}: &str = {v:?};").unwrap();
        }
        writeln!(
            out,
            "const PROVISION_MAX: usize = {};\nconst PROVISION_ROLES: &[(&str,&str)] = &{:?};",
            self.max_peers,
            self.roles.iter().collect::<Vec<_>>()
        )
        .unwrap();
        writeln!(out,"const PROVISION_DEFAULT_RELAYS: &[&str] = &{:?};\nconst PROVISION_ONION_ROUTES: &[&str] = &{:?};", self.default_relays, self.onion_routes).unwrap();
        out.push_str("const MEMBERSHIPS: &[(&str,&str,&str,Option<&str>,&[&str])] = &[\n");
        for s in &self.sets {
            writeln!(
                out,
                "({:?},{:?},{:?},{:?},&{:?}),",
                s.field, s.add, s.remove, s.clear_choice, s.roles
            )
            .unwrap();
        }
        out.push_str("];\n");
        out.push_str(
            &include_str!("provisioning.rs.in").replace("@GLOBAL_GUARDS@", &init.argument_guards()),
        );
        writeln!(out,"pub fn {entry}(matches: clap::ArgMatches) -> std::process::ExitCode {{\n    let run = || -> Result<(), Failure> {{\n        let result = provision_profile(&matches)?;\n        let mut bytes = Vec::new();\n        let mut serializer = serde_json::Serializer::new(&mut bytes);\n        use serde::ser::{{SerializeMap,Serializer}};\n        let mut map = serializer.serialize_map(Some({})).map_err(|_| IO)?;",self.report.len()).unwrap();
        for c in &self.report {
            let value = match &c.source {
                Source::Literal { value } => format!(
                    "serde_json::from_str::<serde_json::Value>({:?}).map_err(|_| BINDING)?",
                    value.to_string()
                ),
                Source::Profile => "result.profile".into(),
                Source::PublicIdentifier => "result.identifier".into(),
                Source::Backend => "PROVISION_FILE".into(),
                Source::Role => "result.config[PROVISION_ROLE_FIELD]".into(),
                Source::AnyMember { field } => {
                    format!("!result.config[{field:?}].as_array().ok_or(CONFIG_FORMAT)?.is_empty()")
                }
                Source::Changed => "result.changed".into(),
                Source::Created => "result.created".into(),
                Source::Opened => "result.opened".into(),
                Source::CleanupComplete => "result.cleanup_complete".into(),
            };
            writeln!(
                out,
                "        map.serialize_entry({:?}, &({value})).map_err(|_| IO)?;",
                c.name
            )
            .unwrap();
        }
        out.push_str("        map.end().map_err(|_| IO)?;\n        bytes.push(b'\\n');\n        std::io::stdout().lock().write_all(&bytes).map_err(|_| IO)\n    };\n    match run() { Ok(()) => std::process::ExitCode::SUCCESS, Err(error) => error.report() }\n}\n");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Provisioning;
    use serde_json::json;

    #[test]
    fn survey_registration_validates_roles_members_and_report_fields() {
        let diagnostic = json!({"exit": 2, "stderr": "invalid survey registration"});
        let graph = json!({
            "id": "urn:survey:register", "command": "enroll",
            "role_argument": "position", "role_field": "position",
            "update_argument": "amend", "interactive_argument": "prompt",
            "default_role": "observer", "roles": {"observer":"observer", "viewer":"observer"},
            "default_relays":["wss://survey.invalid"], "onion_routes":[],
            "sets":[{"field":"panel","add":"invite","remove":"retire","roles":["observer"]}],
            "report":[{"name":"populated","source":{"kind":"any_member","field":"panel"}}],
            "max_peers":8,"peer_format":"lower_hex32",
            "invalid_member":diagnostic,"choice_required":diagnostic,"explicit_removal":diagnostic,
            "existing_config":diagnostic,"unsupported":diagnostic
        });
        let defaults = json!({"position":"observer","panel":[]});
        let validate = |value| {
            serde_json::from_value::<Provisioning>(value)
                .unwrap()
                .validate(
                    "enroll_panel",
                    "initialize",
                    defaults.as_object().unwrap(),
                    &[],
                )
        };
        validate(graph.clone()).unwrap();
        for (pointer, bad) in [
            ("/sets/0/roles/0", "editor"),
            ("/report/0/source/field", "missing"),
            ("/roles/viewer", "unknown"),
            ("/sets/0/add", "position"),
        ] {
            let mut broken = graph.clone();
            *broken.pointer_mut(pointer).unwrap() = json!(bad);
            assert!(validate(broken).is_err(), "{pointer}");
        }
    }
}
