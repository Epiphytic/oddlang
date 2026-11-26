use super::{
    Binding, Callback, RequestArgumentKind, RequiredArgument, arg_id, contains_fractional_literal,
    discovery::Discovery, rust,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::Write,
    fs,
    path::Path,
};

mod profile;
#[allow(dead_code)] // The generated runtime reads all declared effects.
mod schema;
use schema::{ArgumentKind, CaptureKind, Effect, Field, Source, Transition, Workflow};

#[path = "../../strict_json.rs"]
mod strict_json;

const FORMAT: &str = "oddlang-local-command-workflow-v1";

pub(super) fn matches_graph(graph: &[u8]) -> bool {
    serde_json::from_slice::<Value>(graph)
        .is_ok_and(|doc| doc.get("format").and_then(Value::as_str) == Some(FORMAT))
}

fn identity(id: &str, seen: &mut BTreeSet<String>) -> Result<(), String> {
    oxigraph::model::NamedNode::new(id)
        .map_err(|_| format!("invalid workflow declaration identity {id}"))?;
    if !seen.insert(id.to_owned()) {
        return Err(format!("duplicate workflow declaration identity {id}"));
    }
    Ok(())
}

fn argument_kind(kind: ArgumentKind) -> RequestArgumentKind {
    match kind {
        ArgumentKind::String => RequestArgumentKind::String,
        ArgumentKind::RepeatedString => RequestArgumentKind::RepeatedString,
        ArgumentKind::Unsigned64 => RequestArgumentKind::Unsigned64,
        ArgumentKind::Flag => RequestArgumentKind::Flag,
    }
}

fn delivered(transition: &Transition) -> BTreeSet<&str> {
    transition
        .carry
        .iter()
        .map(String::as_str)
        .chain(
            transition
                .capture
                .iter()
                .map(|capture| capture.name.as_str()),
        )
        .collect()
}

fn minimum_request(fields: &[Field], captures: &BTreeMap<&str, &CaptureKind>) -> String {
    let fields = fields
        .iter()
        .map(|field| {
            let key = Value::String(field.key.clone());
            let body = match &field.source {
                Source::Literal { value } => value.to_string(),
                Source::Argument { value_kind, .. } => {
                    argument_kind(*value_kind).minimum_json().to_owned()
                }
                Source::Captured { name } => match captures[name.as_str()] {
                    CaptureKind::LowerHex { bytes } => {
                        format!("\"{}\"", "0".repeat(usize::from(*bytes) * 2))
                    }
                    CaptureKind::String { .. } => "\"\"".into(),
                    CaptureKind::Unsigned64 => "0".into(),
                },
                Source::Input { .. } => "0".into(),
                Source::Object { fields } => minimum_request(fields, captures),
            };
            format!("{key}:{body}")
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", fields.join(","))
}

fn validate_fields(
    fields: &[Field],
    seen: &mut BTreeSet<String>,
    captures: &BTreeSet<&str>,
    inputs: &BTreeSet<&str>,
    arguments: &mut Vec<(String, RequiredArgument)>,
) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    for field in fields {
        identity(&field.id, seen)?;
        if field.key.is_empty() || !keys.insert(&field.key) {
            return Err(format!(
                "duplicate or empty workflow request key {:?}",
                field.key
            ));
        }
        match &field.source {
            Source::Literal { value } => {
                if contains_fractional_literal(value) {
                    return Err("workflow literals require integer JSON numbers".into());
                }
            }
            Source::Argument { id, value_kind } => {
                if !arg_id(id) {
                    return Err(format!("invalid workflow request argument {id}"));
                }
                let kind = argument_kind(*value_kind).required_argument();
                arguments.push((id.to_owned(), kind));
            }
            Source::Captured { name } => {
                if !captures.contains(name.as_str()) {
                    return Err(format!(
                        "workflow capture {name} not assigned on every incoming transition"
                    ));
                }
            }
            Source::Input { name } => {
                if !inputs.contains(name.as_str()) {
                    return Err(format!("undeclared workflow input {name}"));
                }
            }
            Source::Object { fields } => {
                validate_fields(fields, seen, captures, inputs, arguments)?
            }
        }
    }
    Ok(())
}

fn valid_path(path: &[String]) -> bool {
    !path.is_empty() && path.iter().all(|segment| !segment.is_empty())
}

fn diagnostics() -> [(&'static str, &'static str); 8] {
    [
        ("binding", "BINDING"),
        ("io", "IO"),
        ("peer", "PEER"),
        ("frame_size", "FRAME_SIZE"),
        ("frame_json", "FRAME_JSON"),
        ("deadline", "DEADLINE"),
        ("protocol", "PROTOCOL"),
        ("budget", "BUDGET"),
    ]
}

fn validate(
    workflow: &Workflow,
    profile: Option<&profile::ProfileInputs>,
    discovery: Option<&Discovery>,
) -> Result<Vec<(String, RequiredArgument)>, String> {
    let transport = &workflow.transport;
    if workflow.format != FORMAT
        || !arg_id(&workflow.socket_argument)
        || transport.deadline_ms == 0
        || !(1..=16_777_216).contains(&transport.maximum_frame_bytes)
        || transport.maximum_exchanges == 0
    {
        return Err("invalid local workflow transport".into());
    }
    if workflow.errors.len() != diagnostics().len() {
        return Err("wrong number of local workflow diagnostics".into());
    }
    for (key, _) in diagnostics() {
        let error = workflow
            .errors
            .get(key)
            .ok_or_else(|| format!("missing workflow diagnostic {key}"))?;
        if error.exit == 0 || error.stderr.is_empty() || error.stderr.chars().any(char::is_control)
        {
            return Err(format!("invalid workflow diagnostic {key}"));
        }
    }

    let mut seen = BTreeSet::new();
    identity(&workflow.id, &mut seen)?;
    let mut arguments = vec![(workflow.socket_argument.clone(), RequiredArgument::Path)];
    let mut inputs = BTreeSet::new();
    if let Some(profile) = profile {
        let (required, names) = profile::validate(profile, &mut seen)?;
        arguments.extend(required);
        inputs = names;
    }
    if let Some(discovery) = discovery {
        discovery.validate(&workflow.socket_argument)?;
        if let Some(profile) = profile {
            if !discovery.selects_profile(
                &profile.arguments.state,
                &profile.arguments.profile,
                &profile.arguments.default_profile,
            ) {
                return Err(
                    "workflow context and socket discovery select different profiles".into(),
                );
            }
        }
        arguments.extend(discovery.arguments());
    }
    let mut captures = BTreeMap::new();
    for step in &workflow.steps {
        identity(&step.id, &mut seen)?;
        if step.request.is_empty() || step.transitions.is_empty() {
            return Err(format!(
                "empty workflow request or transition list at {}",
                step.id
            ));
        }
        for (position, transition) in step.transitions.iter().enumerate() {
            identity(&transition.id, &mut seen)?;
            if transition.when.is_empty() && position + 1 != step.transitions.len() {
                return Err(format!("workflow fallback must be last at {}", step.id));
            }
            if transition.when.iter().any(|p| {
                !valid_path(&p.path)
                    || p.equals.is_object()
                    || p.equals.is_array()
                    || contains_fractional_literal(&p.equals)
            }) {
                return Err(format!(
                    "invalid workflow reply predicate at {}",
                    transition.id
                ));
            }
            for capture in &transition.capture {
                if !arg_id(&capture.name)
                    || !valid_path(&capture.path)
                    || captures
                        .insert(capture.name.as_str(), &capture.value_kind)
                        .is_some()
                    || matches!(
                        capture.value_kind,
                        CaptureKind::LowerHex { bytes: 0 }
                            | CaptureKind::String { maximum_bytes: 0 }
                    )
                {
                    return Err(format!("invalid workflow capture {}", capture.name));
                }
            }
            if delivered(transition).len() != transition.carry.len() + transition.capture.len() {
                return Err(format!(
                    "duplicate workflow carry or capture at {}",
                    transition.id
                ));
            }
            match &transition.effect {
                Effect::Continue { step, delay_ms } => {
                    if *delay_ms >= transport.deadline_ms
                        || !workflow.steps.iter().any(|found| found.id == *step)
                    {
                        return Err(format!(
                            "invalid workflow target or delay at {}",
                            transition.id
                        ));
                    }
                }
                Effect::Exit { stderr, .. } => {
                    if stderr.chars().any(char::is_control) || !transition.capture.is_empty() {
                        return Err(format!(
                            "invalid workflow terminal effect at {}",
                            transition.id
                        ));
                    }
                }
            }
        }
    }
    let mut pending = VecDeque::from([workflow.entry.as_str()]);
    let mut visited = BTreeSet::new();
    let mut exit_reachable = false;
    while let Some(id) = pending.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        let step = workflow
            .steps
            .iter()
            .find(|step| step.id == id)
            .ok_or("workflow entry does not name a step")?;
        for transition in &step.transitions {
            match &transition.effect {
                Effect::Continue { step, .. } => pending.push_back(step),
                Effect::Exit { .. } => exit_reachable = true,
            }
        }
    }
    if visited.len() != workflow.steps.len() || !exit_reachable {
        return Err("workflow must reach all states and a terminal effect".into());
    }
    if let Some(missing) = workflow
        .steps
        .iter()
        .flat_map(|step| &step.transitions)
        .flat_map(|transition| &transition.carry)
        .find(|name| !captures.contains_key(name.as_str()))
    {
        return Err(format!("undeclared workflow capture {missing}"));
    }
    for step in &workflow.steps {
        let mut guaranteed = None::<BTreeSet<&str>>;
        if step.id == workflow.entry {
            guaranteed = Some(BTreeSet::new());
        }
        for incoming in workflow.steps.iter().flat_map(|source| &source.transitions) {
            if matches!(&incoming.effect, Effect::Continue { step: target, .. } if target == &step.id)
            {
                let names = delivered(incoming);
                guaranteed = Some(match guaranteed {
                    Some(previous) => previous.intersection(&names).copied().collect(),
                    None => names,
                });
            }
        }
        let guaranteed = guaranteed.expect("every validated step is reachable");
        if let Some(missing) = step
            .transitions
            .iter()
            .flat_map(|transition| &transition.carry)
            .find(|name| !guaranteed.contains(name.as_str()))
        {
            return Err(format!(
                "cannot carry workflow capture {missing} before assignment"
            ));
        }
        validate_fields(
            &step.request,
            &mut seen,
            &guaranteed,
            &inputs,
            &mut arguments,
        )?;
        if minimum_request(&step.request, &captures).len() > transport.maximum_frame_bytes as usize
        {
            return Err(format!(
                "minimum workflow request exceeds frame limit at {}",
                step.id
            ));
        }
    }
    Ok(arguments)
}

struct SelectedWorkflow {
    workflow: Workflow,
    binding: Binding,
    profile: Option<profile::ProfileInputs>,
    discovery: Option<Discovery>,
    arguments: Vec<(String, RequiredArgument)>,
}

fn load(graph: &[u8], binding: &[u8]) -> Result<SelectedWorkflow, String> {
    let strict_json::Strict(value) =
        serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let workflow: Workflow = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let binding: Binding = serde_json::from_slice(binding).map_err(|error| error.to_string())?;
    if binding.format != "oddlang-local-command-client-rust-v1"
        || !super::valid_entry(&binding.entry)
        || [
            "field_at",
            "explicit_argument",
            "argument_value",
            "append",
            "request",
            "capture_is_valid",
            "run_workflow",
            "execute_workflow",
            "initial_inputs",
            "choose_socket",
        ]
        .contains(&binding.entry.as_str())
    {
        return Err("invalid local command workflow entry".into());
    }
    let profile = match (
        &workflow.profile_inputs,
        binding.profile_provider.as_deref(),
    ) {
        (Some(profile), Some("nostr-url-base64-unix")) => Some(profile::load(profile)?),
        (None, None) => None,
        _ => {
            return Err(
                "workflow private profile inputs require their pinned provider binding".into(),
            );
        }
    };
    let discovery = workflow
        .discovery
        .clone()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?;
    if (profile.is_some() && profile::reserved_entry(&binding.entry))
        || (discovery.is_some()
            && (binding.entry == "discover_endpoint" || binding.entry.starts_with("endpoint_")))
    {
        return Err("invalid local command workflow entry".into());
    }
    let arguments = validate(&workflow, profile.as_ref(), discovery.as_ref())?;
    Ok(SelectedWorkflow {
        workflow,
        binding,
        profile,
        discovery,
        arguments,
    })
}

pub(super) fn callback(graph: &[u8], binding: &[u8]) -> Result<Callback, String> {
    let selected = load(graph, binding)?;
    Ok(Callback {
        local_arguments: None,
        operation: None,
        entry: selected.binding.entry,
        arguments: selected.arguments,
    })
}

fn field_obligations(fields: &[Field], rows: &mut Vec<Value>) {
    for (position, field) in fields.iter().enumerate() {
        let mut row = json!({
            "declaration": field.id, "role": "workflow-request-field", "position": position, "key": field.key,
        });
        match &field.source {
            Source::Argument { id, value_kind } => {
                row["explicitArgument"] = json!(id);
                row["representation"] = argument_kind(*value_kind).representation();
            }
            Source::Captured { name } => row["captured"] = json!(name),
            Source::Input { name } => row["authorizedInput"] = json!(name),
            Source::Literal { value } => row["literal"] = json!(value),
            Source::Object { fields: children } => {
                let mut nested = Vec::new();
                field_obligations(children, &mut nested);
                row["orderedFields"] = json!(nested);
            }
        }
        rows.push(row);
    }
}

pub(super) fn build(graph: &[u8], binding: &[u8], output: &Path) -> Result<(), String> {
    let selected = load(graph, binding)?;
    let workflow = selected.workflow;
    let mut source =
        String::from("// Generated by Oddlang from oddlang-local-command-workflow-v1.\n");
    source.push_str(&rust::framed_exchange(
        ", request: &[u8]",
        "request.len()",
        "request",
    ));
    writeln!(
        source,
        "\nmod schema {{\n#![allow(dead_code)]\n{}\n}}",
        include_str!("schema.rs")
    )
    .unwrap();
    writeln!(
        source,
        "\nmod strict_json {{\n{}\n}}",
        include_str!("../../strict_json.rs")
    )
    .unwrap();
    let policy: Value = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let encoded = serde_json::to_string(&policy).map_err(|error| error.to_string())?;
    writeln!(source, "const WORKFLOW: &str = {encoded:?};").unwrap();
    writeln!(
        source,
        "const FRAME_LIMIT: u32 = {};",
        workflow.transport.maximum_frame_bytes
    )
    .unwrap();
    writeln!(
        source,
        "const DEADLINE_MS: u64 = {};",
        workflow.transport.deadline_ms
    )
    .unwrap();
    for (key, constant) in diagnostics() {
        let error = &workflow.errors[key];
        writeln!(
            source,
            "const {constant}: Failure = Failure({}, {:?});",
            error.exit, error.stderr
        )
        .unwrap();
    }
    if let Some(profile) = &selected.profile {
        profile::render(profile, &mut source);
        source.push_str(r#"
fn initial_inputs(workflow: &Workflow, matches: &ArgMatches) -> Result<BTreeMap<String, Value>, Failure> {
    let raw = workflow.profile_inputs.as_ref().ok_or(PROTOCOL)?;
    let policy = serde_json::from_value(raw.clone()).map_err(|_| PROTOCOL)?;
    context_values(&policy, matches)
}
"#);
    } else {
        source.push_str(
            r#"
fn initial_inputs(_: &Workflow, _: &ArgMatches) -> Result<BTreeMap<String, Value>, Failure> {
    Ok(BTreeMap::new())
}
"#,
        );
    }
    if let Some(discovery) = &selected.discovery {
        discovery.render(&mut source);
        if let Some(profile) = &selected.profile {
            let profile_argument = &profile.arguments.profile;
            writeln!(
                source,
                r#"
fn choose_socket(matches: &ArgMatches, explicit: Option<&PathBuf>) -> Result<PathBuf, Failure> {{
    let mut selected = matches.clone();
    if !explicit_argument(matches, {profile_argument:?}) {{
        selected.try_remove_one::<String>({profile_argument:?}).map_err(|_| BINDING)?;
    }}
    discover_endpoint(&selected, explicit)
}}
"#
            )
            .unwrap();
        } else {
            source.push_str(
                r#"
fn choose_socket(matches: &ArgMatches, explicit: Option<&PathBuf>) -> Result<PathBuf, Failure> {
    discover_endpoint(matches, explicit)
}
"#,
            );
        }
    } else {
        source.push_str(
            r#"
fn choose_socket(_: &ArgMatches, explicit: Option<&PathBuf>) -> Result<PathBuf, Failure> {
    explicit.cloned().ok_or(BINDING)
}
"#,
        );
    }
    source.push_str(include_str!("runtime.rs.in"));
    let entry = selected.binding.entry;
    writeln!(source, "\npub fn {entry}(matches: ArgMatches) -> ExitCode {{\n    match execute_workflow(&matches) {{\n        Ok(code) => ExitCode::from(code),\n        Err(error) => error.report(),\n    }}\n}}").unwrap();

    let rows = workflow
        .steps
        .iter()
        .map(|step| {
            let mut ordered_fields = Vec::new();
            field_obligations(&step.request, &mut ordered_fields);
            json!({"declaration": step.id, "role": "workflow-step", "request": ordered_fields})
        })
        .collect::<Vec<_>>();
    let obligations = format!(
        "{:#}\n",
        json!({
            "format": "oddlang-local-workflow-obligations-v1",
            "declaration": workflow.id,
            "role": "same-user-local-command-client",
            "qualification": "independent-witness-required",
            "framing": "u32_be_json_half_close",
            "encoding": "compactUtf8Json",
            "objectOrder": "declarationOrder",
            "literalObjectOrder": "lexicographicUtf8Keys",
            "literalNumberEncoding": "signedOrUnsignedDecimalInteger",
            "stringEscaping": {
                "quote": "backslash", "reverseSolidus": "backslash", "solidus": "raw",
                "controlRange": "U+0000-U+001F", "shortControls": ["b", "t", "n", "f", "r"],
                "otherControls": "lowercase-u00xx", "nonAscii": "rawUtf8",
            },
            "replyDuplicateMembers": "reject",
            "captureLifetime": "nextStepUnlessExplicitlyCarried",
            "effects": policy,
            "obligations": rows,
        })
    );
    use crate::wire_schema::digest;
    let mut lock = json!({
        "format": "oddlang-local-command-workflow-lock-v1",
        "graphSha256": digest(graph),
        "bindingSha256": digest(binding),
        "sourceSha256": digest(source.as_bytes()),
        "workflowObligationsSha256": digest(obligations.as_bytes()),
    });
    let profile_obligations = workflow
        .profile_inputs
        .as_ref()
        .map(|profile| format!("{:#}\n", profile::obligations(profile)));
    if let Some(catalog) = &profile_obligations {
        lock["profileObligationsSha256"] = digest(catalog.as_bytes()).into();
    }
    let endpoint_obligations = selected.discovery.as_ref().map(|discovery| {
        let mut catalog = discovery.obligations();
        if selected.profile.is_some() {
            if let Some(order) = catalog["selectionOrder"].as_array_mut() {
                order.retain(|source| source != "ordered-environment");
            }
            catalog["composedStateSelection"] = "explicitStateRequired".into();
        }
        format!("{catalog:#}\n")
    });
    if let Some(discovery) = &selected.discovery {
        lock["discoverySha256"] =
            digest(&serde_json::to_vec(discovery).map_err(|error| error.to_string())?).into();
    }
    if let Some(catalog) = &endpoint_obligations {
        lock["endpointObligationsSha256"] = digest(catalog.as_bytes()).into();
    }
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    for (name, content) in [
        ("client.rs", source),
        ("projection.lock", format!("{lock:#}\n")),
        ("workflow-obligations.json", obligations),
    ] {
        fs::write(stage.path().join(name), content).map_err(|error| error.to_string())?;
    }
    for (name, catalog) in [
        ("profile-obligations.json", profile_obligations),
        ("endpoint-obligations.json", endpoint_obligations),
    ] {
        if let Some(content) = catalog {
            fs::write(stage.path().join(name), content).map_err(|error| error.to_string())?;
        }
    }
    crate::native_package::publish(stage.path(), output)
}
