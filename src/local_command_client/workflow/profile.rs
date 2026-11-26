use super::{RequiredArgument, arg_id, argument_kind, identity, schema};
use serde_json::Value;
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

const PAYLOAD_ERRORS: [(&str, &str); 2] = [
    ("stdin_timeout", "PROFILE_STDIN_TIMEOUT"),
    ("random", "PROFILE_RANDOM"),
];

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
        if !matches!(rule.scheme.as_str(), "ws" | "wss")
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
        if !checked.insert(&check.field)
            || check.field.is_empty()
            || matches!(&check.test, InputTest::OneOf { values } if values.is_empty()
                || values.iter().any(|value| value.is_array() || value.is_object()))
        {
            return Err("invalid workflow profile document check".into());
        }
    }
    let diagnostics = diagnostics().into_iter().chain(PAYLOAD_ERRORS);
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
    let names = [
        &out.remote,
        &out.relays,
        &out.argv,
        &out.stdin,
        &out.request,
        &out.invocation,
    ]
    .into_iter()
    .map(String::as_str)
    .collect::<BTreeSet<_>>();
    if names.len() != 6 || names.iter().any(|name| !arg_id(name)) {
        return Err("invalid or duplicate configured workflow output name".into());
    }
    Ok((arguments, names))
}

pub(super) fn render(policy: &ProfileInputs, out: &mut String) {
    writeln!(
        out,
        "\nmod profile_schema {{\n#![allow(dead_code)]\n{}\n}}",
        include_str!("profile_schema.rs")
    )
    .unwrap();
    for (key, constant) in diagnostics().into_iter().chain(PAYLOAD_ERRORS) {
        let error = &policy.errors[key];
        writeln!(
            out,
            "const {constant}: Failure = Failure({}, {:?});",
            error.exit, error.stderr
        )
        .unwrap();
    }
    out.push_str(include_str!("profile_runtime.rs.in"));
}
