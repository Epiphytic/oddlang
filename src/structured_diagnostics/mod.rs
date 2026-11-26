//! Project a bounded program of state-gated structured observations.
use crate::wire_schema::{digest, encode, symbol};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

mod recover;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    flag: Flag,
    functions: Vec<Function>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Flag {
    name: String,
    initial: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Function {
    name: String,
    parameters: Vec<Parameter>,
    actions: Vec<Action>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    name: String,
    writable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Action {
    Set {
        value: bool,
    },
    Assign {
        target: String,
        value: Text,
    },
    When {
        actions: Vec<Action>,
    },
    Emit {
        #[serde(deserialize_with = "unique_fields")]
        fields: BTreeMap<String, Value>,
    },
    Call {
        function: String,
        arguments: Vec<Text>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Text {
    Literal { value: String },
    Parameter { name: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Value {
    Text { source: Text },
    Unsigned { value: u64 },
    Boolean { value: bool },
    HostOs,
    HostArchitecture,
    PackageVersion,
}

fn unique_fields<'de, D>(deserializer: D) -> std::result::Result<BTreeMap<String, Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Fields;

    impl<'de> serde::de::Visitor<'de> for Fields {
        type Value = BTreeMap<String, Value>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a diagnostic object with unique field names")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut fields = BTreeMap::new();
            while let Some((name, value)) = map.next_entry::<String, Value>()? {
                if fields.insert(name.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate record field {name}"
                    )));
                }
            }
            Ok(fields)
        }
    }
    deserializer.deserialize_map(Fields)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    sink: Sink,
    borrow: Borrow,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Sink {
    Stderr,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Borrow {
    Static,
}

const PROFILE: &str = "oddlang-structured-diagnostics-v1";

type Result<T> = std::result::Result<T, String>;

fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}

pub fn recover(source: &Path, output: &Path) -> Result<()> {
    let graph = recover::extract(&io(fs::read_to_string(source))?)?;
    validate(&graph)?;
    io(fs::write(output, encode(&graph)?))
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    build_bytes(&io(fs::read(graph))?, &io(fs::read(overlay))?, output)
}

pub(crate) fn build_bytes(graph: &[u8], overlay: &[u8], output: &Path) -> Result<()> {
    let spec: Graph = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let binding: Overlay = serde_json::from_slice(overlay).map_err(|error| error.to_string())?;
    validate(&spec)?;
    let source = render(&spec, &binding);
    let lock = serde_json::json!({
        "format": "oddlang-structured-diagnostics-lock-v1",
        "graphSha256": digest(graph),
        "overlaySha256": digest(overlay),
        "sourceSha256": digest(source.as_bytes()),
    });
    io(fs::create_dir(output))?;
    io(fs::write(output.join("diagnostics.rs"), source))?;
    io(fs::write(output.join("projection.lock"), encode(&lock)?))
}

fn validate(graph: &Graph) -> Result<()> {
    if graph.format != PROFILE || !symbol(&graph.flag.name) || graph.functions.is_empty() {
        return Err("invalid structured diagnostics graph".into());
    }
    let mut callable = BTreeMap::new();
    let mut used = BTreeSet::from([graph.flag.name.as_str()]);
    for function in &graph.functions {
        let mut parameters = BTreeMap::new();
        if !symbol(&function.name) || !used.insert(&function.name) {
            return Err(format!("invalid or duplicate function {}", function.name));
        }
        for parameter in &function.parameters {
            // json! interprets this unqualified token as a null before resolving an argument.
            if !symbol(&parameter.name)
                || parameter.name == "null"
                || parameter.name == graph.flag.name
                || parameters
                    .insert(parameter.name.as_str(), parameter.writable)
                    .is_some()
            {
                return Err(format!("invalid or duplicate parameter {}", parameter.name));
            }
        }
        validate_actions(&function.actions, &parameters, &callable, 0)?;
        if function
            .parameters
            .iter()
            .all(|parameter| !parameter.writable)
        {
            callable.insert(function.name.as_str(), function.parameters.len());
        }
    }
    Ok(())
}

fn validate_actions(
    actions: &[Action],
    parameters: &BTreeMap<&str, bool>,
    callable: &BTreeMap<&str, usize>,
    depth: usize,
) -> Result<()> {
    if depth > 12 || actions.is_empty() {
        return Err("actions must be nonempty with bounded guard nesting".into());
    }
    let check_text = |text: &Text| match text {
        Text::Literal { .. } => Ok(()),
        Text::Parameter { name } if parameters.get(name.as_str()) == Some(&false) => Ok(()),
        Text::Parameter { name } => Err(format!("expected read-only text parameter {name}")),
    };
    for action in actions {
        match action {
            Action::Set { .. } => (),
            Action::Assign { target, value } => {
                if parameters.get(target.as_str()) != Some(&true) {
                    return Err(format!("expected writable text parameter {target}"));
                }
                check_text(value)?;
            }
            Action::When { actions } => validate_actions(actions, parameters, callable, depth + 1)?,
            Action::Emit { fields } => {
                if fields.is_empty() {
                    return Err("cannot emit an empty diagnostic record".into());
                }
                for field in fields.values() {
                    if let Value::Text { source } = field {
                        check_text(source)?;
                    }
                }
            }
            Action::Call {
                function,
                arguments,
            } => {
                if parameters.contains_key(function.as_str())
                    || callable.get(function.as_str()) != Some(&arguments.len())
                {
                    return Err(format!(
                        "expected earlier compatible text function {function}"
                    ));
                }
                for argument in arguments {
                    check_text(argument)?;
                }
            }
        }
    }
    Ok(())
}

fn render(graph: &Graph, overlay: &Overlay) -> String {
    let mut out = String::new();
    let borrowed = match overlay.borrow {
        Borrow::Static => "&'static str",
    };
    let sink = match overlay.sink {
        Sink::Stderr => "::std::io::stderr()",
    };
    writeln!(
        out,
        "use ::std::io::Write as _;\nstatic {}: ::std::sync::atomic::AtomicBool = ::std::sync::atomic::AtomicBool::new({});",
        graph.flag.name, graph.flag.initial
    ).unwrap();
    for function in &graph.functions {
        let arguments = function
            .parameters
            .iter()
            .map(|p| {
                let ty = if p.writable {
                    format!("&mut {borrowed}")
                } else {
                    borrowed.to_owned()
                };
                format!("{}: {ty}", p.name)
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "\npub fn {}({arguments}) {{", function.name).unwrap();
        render_actions(&mut out, &function.actions, &graph.flag.name, sink, 1);
        out.push_str("}\n");
    }
    out
}

fn render_text(text: &Text) -> String {
    match text {
        Text::Literal { value } => format!("{value:?}"),
        Text::Parameter { name } => name.clone(),
    }
}

fn render_actions(out: &mut String, actions: &[Action], flag: &str, sink: &str, depth: usize) {
    let indent = "    ".repeat(depth);
    let ordering = "::std::sync::atomic::Ordering::Relaxed";
    for action in actions {
        match action {
            Action::Set { value } => {
                writeln!(out, "{indent}self::{flag}.store({value}, {ordering});").unwrap();
            }
            Action::Assign { target, value } => {
                writeln!(out, "{indent}*{target} = {};", render_text(value)).unwrap();
            }
            Action::When { actions } => {
                writeln!(out, "{indent}if self::{flag}.load({ordering}) {{").unwrap();
                render_actions(out, actions, flag, sink, depth + 1);
                writeln!(out, "{indent}}}").unwrap();
            }
            Action::Emit { fields } => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| {
                        let source = match value {
                            Value::Text { source } => render_text(source),
                            Value::Unsigned { value } => format!("{value}u64"),
                            Value::Boolean { value } => value.to_string(),
                            Value::HostOs => "::std::env::consts::OS".into(),
                            Value::HostArchitecture => "::std::env::consts::ARCH".into(),
                            Value::PackageVersion => "::std::env!(\"CARGO_PKG_VERSION\")".into(),
                        };
                        format!("{key:?}: {source}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(out, "{indent}let _ = ::std::writeln!({sink}.lock(), \"{{}}\", ::serde_json::json!({{{fields}}}));").unwrap();
            }
            Action::Call {
                function,
                arguments,
            } => {
                let arguments = arguments
                    .iter()
                    .map(render_text)
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(out, "{indent}self::{function}({arguments});").unwrap();
            }
        }
    }
}
