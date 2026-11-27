use super::{RequiredArgument, arg_id, argument_kind, identity, schema};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt::Write};

#[allow(dead_code)] // The generated native loader executes these declarations.
#[path = "profile_schema.rs"]
mod profile_schema;
use profile_schema::InputTest;
pub(super) use profile_schema::ProfileInputs;

fn diagnostics() -> [(&'static str, &'static str); 11] {
    [
        ("scope", "PROFILE_SCOPE"),
        ("path", "PROFILE_PATH"),
        ("file", "PROFILE_FILE"),
        ("size", "PROFILE_SIZE"),
        ("schema", "PROFILE_SCHEMA"),
        ("destination", "PROFILE_DESTINATION"),
        ("authority", "PROFILE_AUTHORITY"),
        ("relays", "PROFILE_RELAYS"),
        ("separator", "PROFILE_SEPARATOR"),
        ("argv", "PROFILE_ARGV"),
        ("bytes", "PROFILE_BYTES"),
    ]
}

// Older contracts use the path and file diagnostics for all open failures.
const OPTIONAL_READ_ERRORS: [(&str, &str); 4] = [
    ("state_directory", "PROFILE_STATE_DIRECTORY"),
    ("missing_profile_directory", "PROFILE_MISSING_DIRECTORY"),
    ("document_open", "PROFILE_DOCUMENT_OPEN"),
    ("document_metadata", "PROFILE_DOCUMENT_METADATA"),
];

const PAYLOAD_ERRORS: [(&str, &str); 2] = [
    ("stdin_timeout", "PROFILE_STDIN_TIMEOUT"),
    ("random", "PROFILE_RANDOM"),
];

fn configured_diagnostics(
    policy: &ProfileInputs,
) -> impl Iterator<Item = (&'static str, &'static str)> + Clone + '_ {
    diagnostics().into_iter().chain(PAYLOAD_ERRORS).chain(
        OPTIONAL_READ_ERRORS
            .into_iter()
            .filter(move |(name, _)| policy.errors.contains_key(*name)),
    )
}

pub(super) fn load(value: &Value) -> Result<ProfileInputs, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

pub(super) fn validate<'a>(
    policy: &'a ProfileInputs,
    seen: &mut BTreeSet<String>,
) -> Result<(Vec<(String, RequiredArgument)>, BTreeSet<&'a str>), String> {
    identity(&policy.id, seen)?;
    let args = &policy.arguments;
    let doc = &policy.document;
    let payload = &policy.payload;
    if !arg_id(&args.default_profile)
        || !arg_id(&doc.leaf.replace('.', "-"))
        || matches!(doc.leaf.as_str(), "." | "..")
        || doc.profile_name_maximum_bytes == 0
        || doc
            .profile_name_punctuation
            .bytes()
            .any(|b| !matches!(b, b'_' | b'-'))
        || !(1..=16_777_216).contains(&doc.maximum_bytes)
        || doc.directory_mode != 0o700
        || doc.file_mode != 0o600
        || doc.maximum_accounts == 0
        || doc.maximum_relays == 0
        || doc.maximum_relay_bytes == 0
        || doc.relay_rules.is_empty()
        || payload.maximum_arguments == 0
        || !(1..=16_777_216).contains(&payload.maximum_argument_bytes)
        || !(1..=16_777_216).contains(&payload.maximum_stdin_bytes)
        || payload.stdin_timeout_ms == 0
        || !(1..=8).contains(&payload.timestamp_bytes)
        || !(8..=64).contains(&payload.random_bytes)
    {
        return Err("invalid configured workflow profile or payload bounds".into());
    }
    let mut checked = BTreeSet::from([&doc.trusted, &doc.relay, &doc.route]);
    if checked.len() != 3 || checked.iter().any(|field| field.is_empty()) {
        return Err("workflow profile fields must have distinct names".into());
    }
    for rule in &doc.relay_rules {
        identity(&rule.id, seen)?;
        if !matches!(rule.scheme.as_str(), "ws" | "wss")
            || (rule.v3_onion && rule.host_suffix.as_deref() != Some(".onion"))
            || rule.routes.is_empty()
            || rule.routes.iter().any(|route| route.is_empty())
            || rule
                .host_suffix
                .as_ref()
                .is_some_and(|suffix| !suffix.starts_with('.') || suffix.len() < 2)
        {
            return Err("invalid workflow profile relay rule".into());
        }
    }
    for check in &doc.checks {
        identity(&check.id, seen)?;
        if !checked.insert(&check.field)
            || check.field.is_empty()
            || matches!(&check.test, InputTest::OneOf { values } if values.is_empty()
                || values.iter().any(|value| value.is_array() || value.is_object()))
        {
            return Err("invalid workflow profile document check".into());
        }
    }
    let diagnostics = configured_diagnostics(policy);
    if policy.errors.len() != diagnostics.clone().count()
        || diagnostics.into_iter().any(|(field, _)| {
            policy.errors.get(field).is_none_or(|diagnostic| {
                diagnostic.exit == 0
                    || diagnostic.stderr.is_empty()
                    || diagnostic.stderr.chars().any(char::is_control)
            })
        })
    {
        return Err("invalid workflow profile diagnostics".into());
    }
    let mut arguments = vec![
        (args.state.clone(), RequiredArgument::Path),
        (args.profile.clone(), RequiredArgument::String),
        (args.relay.clone(), RequiredArgument::RepeatedString),
        (args.route.clone(), RequiredArgument::String),
        (args.stdin.clone(), RequiredArgument::Flag),
        (args.stdin_none.clone(), RequiredArgument::Flag),
    ];
    arguments.extend(
        args.required_flags
            .iter()
            .chain(&args.excluded_flags)
            .map(|flag| (flag.clone(), RequiredArgument::Flag)),
    );
    arguments.extend(args.excluded_values.iter().map(|arg| {
        (
            arg.id.clone(),
            argument_kind(arg.value_kind).required_argument(),
        )
    }));
    if arguments.iter().any(|(argument, _)| !arg_id(argument)) {
        return Err("invalid input context argument identity".into());
    }
    let out = &policy.outputs;
    let mut names = vec![&out.remote, &out.invocation];
    if let Some(resume) = &policy.resume {
        identity(&resume.id, seen)?;
        oxigraph::model::NamedNode::new(&resume.operation)
            .map_err(|_| "invalid resume operation identity")?;
        let keys = &resume.handle_fields;
        let manifest = &resume.manifest;
        let handle_keys = BTreeSet::from([
            &keys.version_key,
            &keys.profile,
            &keys.remote,
            &keys.session,
            &keys.invocation,
            &keys.digest,
        ]);
        let manifest_keys = BTreeSet::from([
            &manifest.version_key,
            &manifest.name_key,
            &manifest.phase_key,
            &manifest.account_key,
        ]);
        if !arg_id(&resume.command)
            || !arg_id(&resume.handle_argument)
            || !(1..=4096).contains(&resume.maximum_handle_bytes)
            || handle_keys.len() != 6
            || handle_keys.iter().any(|key| key.is_empty())
            || manifest_keys.len() != 4
            || manifest_keys.iter().any(|key| key.is_empty())
            || !arg_id(&manifest.leaf.replace('.', "-"))
            || matches!(manifest.leaf.as_str(), "." | "..")
            || !(1..=65_536).contains(&manifest.maximum_bytes)
            || manifest.ready.is_empty()
            || out.selected_profile.is_none()
            || out.followup_request.is_some()
        {
            return Err("invalid workflow resume policy".into());
        }
        names.extend([
            &resume.local_account_output,
            &resume.session_output,
            &resume.digest_output,
        ]);
    } else {
        names.extend([&out.relays, &out.argv, &out.stdin, &out.request]);
    }
    names.extend(out.selected_profile.iter());
    names.extend(out.followup_request.iter());
    let count = names.len();
    let names = names
        .into_iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if names.len() != count || names.iter().any(|name| !arg_id(name)) {
        return Err("invalid or duplicate configured workflow output name".into());
    }
    Ok((arguments, names))
}

pub(super) fn reserved_entry(entry: &str) -> bool {
    [
        "private_openat",
        "private_metadata",
        "get_account",
        "lower_hex",
        "account_set",
        "canonical_relays",
        "valid_v3_onion",
        "timestamped_id",
        "profile_flag",
        "read_command_stdin",
        "load_profile_document",
        "context_values",
        "resume_inputs",
    ]
    .contains(&entry)
}

pub(super) fn obligations(policy: &Value) -> Value {
    let document = &policy["document"];
    let mut rows = vec![json!({
        "declaration": policy["id"], "role": "authorized-workflow-inputs",
        "argumentSource": "explicitCommandLine",
        "stateSelection": "explicitStateRequired",
        "profileSelection": "explicitProfileOrDeclaredDefault",
        "documentAccess": "readOnlyDescriptorRelativeNoFollow",
        "relayOrder": "canonicalLexicographicUnique",
        "profilePolicy": policy,
    })];
    for rule in document["checks"].as_array().into_iter().flatten() {
        rows.push(
            json!({"declaration": rule["id"], "role": "private-document-check", "value": rule}),
        );
    }
    for rule in document["relay_rules"].as_array().into_iter().flatten() {
        rows.push(
            json!({"declaration": rule["id"], "role": "authorized-relay-rule", "value": rule}),
        );
    }
    json!({
        "format": "oddlang-private-workflow-input-obligations-v1",
        "qualification": "independent-witness-required",
        "obligations": rows,
    })
}

pub(super) fn render(policy: &ProfileInputs, out: &mut String) {
    writeln!(
        out,
        "\nmod profile_schema {{\n#![allow(dead_code)]\n{}\n}}",
        include_str!("profile_schema.rs")
    )
    .unwrap();
    for (key, constant) in configured_diagnostics(policy) {
        let error = &policy.errors[key];
        writeln!(
            out,
            "const {constant}: Failure = Failure({}, {:?});",
            error.exit, error.stderr
        )
        .unwrap();
    }
    let profile_open = if policy.errors.contains_key("missing_profile_directory") {
        "|error| if error.kind() == io::ErrorKind::NotFound { PROFILE_MISSING_DIRECTORY } else { PROFILE_PATH }"
    } else {
        "|_| PROFILE_PATH"
    };
    let mut runtime =
        include_str!("profile_runtime.rs.in").replace("/*profile_open_failure*/", profile_open);
    for (marker, key, selected, fallback) in [
        (
            "/*state_directory_error*/",
            "state_directory",
            "PROFILE_STATE_DIRECTORY",
            "PROFILE_PATH",
        ),
        (
            "/*document_open_error*/",
            "document_open",
            "PROFILE_DOCUMENT_OPEN",
            "PROFILE_FILE",
        ),
        (
            "/*document_metadata_error*/",
            "document_metadata",
            "PROFILE_DOCUMENT_METADATA",
            "PROFILE_FILE",
        ),
    ] {
        runtime = runtime.replace(
            marker,
            if policy.errors.contains_key(key) {
                selected
            } else {
                fallback
            },
        );
    }
    out.push_str(&runtime);
}

#[cfg(test)]
mod tests {
    use super::*;

    // A meter station uses its own error vocabulary for calibration profiles.
    fn meter() -> Value {
        let mut errors = serde_json::Map::new();
        for (key, _) in diagnostics().into_iter().chain(PAYLOAD_ERRORS) {
            errors.insert(key.to_owned(), json!({"exit": 6, "stderr": key}));
        }
        json!({
            "id": "urn:meter:calibration",
            "arguments": {"state":"store","profile":"station","default_profile":"bench",
                "relay":"endpoint","route":"network","stdin":"input","stdin_none":"empty",
                "required_flags":[],"excluded_flags":[],"excluded_values":[]},
            "document": {"leaf":"calibration.json","directory_mode":448,"file_mode":384,
                "maximum_bytes":1024,"profile_name_maximum_bytes":24,"profile_name_punctuation":"-",
                "checks":[],"trusted":"operators","maximum_accounts":2,"relay":"endpoints","route":"network",
                "maximum_relays":2,"maximum_relay_bytes":512,
                "relay_rules":[{"id":"urn:meter:relay","scheme":"wss","routes":["lan"]}]},
            "payload": {"maximum_arguments":2,"maximum_argument_bytes":32,"maximum_stdin_bytes":64,
                "stdin_timeout_ms":100,"timestamp_bytes":4,"random_bytes":8},
            "outputs": {"remote":"operator","relays":"endpoints","argv":"reading","stdin":"sample",
                "request":"calibration","invocation":"sequence"},
            "errors": errors
        })
    }

    #[test]
    fn calibration_read_errors_are_independently_selectable() {
        for (key, constant) in OPTIONAL_READ_ERRORS {
            let mut graph = meter();
            graph["errors"][key] = json!({"exit": 27, "stderr": "station unavailable"});
            let policy = load(&graph).unwrap();
            validate(&policy, &mut BTreeSet::new()).unwrap();
            let mut source = String::new();
            render(&policy, &mut source);
            assert!(source.contains(&format!(
                "const {constant}: Failure = Failure(27, \"station unavailable\");"
            )));
            assert!(source.matches(constant).count() >= 2);
            for (_, other) in OPTIONAL_READ_ERRORS {
                if other != constant {
                    assert!(!source.contains(other));
                }
            }

            graph["errors"][key]["exit"] = json!(0);
            assert!(validate(&load(&graph).unwrap(), &mut BTreeSet::new()).is_err());
        }
    }
}
