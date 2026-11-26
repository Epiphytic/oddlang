use super::{Diagnostic, RequiredArgument, arg_id, unique_keys};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::Write,
    path::{Component, Path},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Discovery {
    id: String,
    state_argument: String,
    profile_argument: String,
    default_profile: String,
    state_sources: Vec<StateSource>,
    missing_state: Diagnostic,
    profile_atom: Atom,
    private_directory_mode: u16,
    private_file_mode: u16,
    unsafe_directory: Diagnostic,
    unsafe_file: Diagnostic,
    profile: Document,
    binding: Document,
    runtime: Vec<RuntimePart>,
    socket_leaf: String,
    socket_maximum_exclusive: u16,
    invalid_runtime_component: Diagnostic,
    socket_too_long: Diagnostic,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateSource {
    variable: String,
    platform: Option<Platform>,
    require_absolute: bool,
    suffix: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Linux,
    Macos,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Atom {
    maximum_bytes: u16,
    punctuation: String,
    invalid: Diagnostic,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    id: String,
    leaf: String,
    maximum_bytes: u32,
    overflow: Overflow,
    missing: Option<Diagnostic>,
    invalid: Diagnostic,
    fields: Vec<Field>,
    checks: Vec<Check>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Overflow {
    Reject,
    BoundedPrefix,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    id: String,
    key: String,
    #[serde(rename = "type")]
    ty: FieldType,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum FieldType {
    Text,
    NullableText,
    UnsignedByte,
    Strings { values: Vec<String> },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: String,
    when: Option<Predicate>,
    require: Predicate,
    rejected: Diagnostic,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Predicate {
    Equal {
        left: Operand,
        right: Operand,
    },
    Different {
        left: Operand,
        right: Operand,
    },
    LowerHex {
        value: Operand,
        bytes: u8,
        nullable: bool,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operand {
    ProfileField { id: String },
    BindingField { id: String },
    ProfileName,
    ProfilePath,
    Text { value: String },
    UnsignedByte { value: u8 },
    Null,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RuntimePart {
    Literal { value: String },
    EffectiveUid,
    ProfileField { id: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AtomType {
    Text,
    Nullable,
    Unsigned,
    Null,
    Path,
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['/', '\0'])
        && value != "."
        && value != ".."
        && matches!(Path::new(value).components().next(), Some(Component::Normal(part)) if part == value)
}

fn iri(id: &str, seen: &mut BTreeSet<String>) -> Result<(), String> {
    oxigraph::model::NamedNode::new(id)
        .map_err(|_| format!("invalid endpoint discovery identity {id}"))?;
    if !seen.insert(id.to_owned()) {
        return Err(format!("duplicate endpoint discovery identity {id}"));
    }
    Ok(())
}

impl Discovery {
    pub(super) fn selects_profile(&self, state: &str, profile: &str, default: &str) -> bool {
        self.state_argument == state
            && self.profile_argument == profile
            && self.default_profile == default
    }

    pub(super) fn obligations(&self) -> serde_json::Value {
        serde_json::json!({
            "format": "oddlang-local-endpoint-obligations-v1",
            "declaration": self.id,
            "selectionOrder": ["explicit-state", "ordered-environment", "explicit-endpoint",
                "profile-name", "private-state-directory", "canonical-state",
                "private-profile-directory", "profile-document", "ordered-profile-checks",
                "native-temporary-directory", "private-runtime-directory", "binding-document",
                "ordered-binding-checks", "endpoint-byte-width"],
            "directory": {"owner":"effective-uid", "requireDirectory":true, "followFinalSymlink":false, "permissionMask":511},
            "file": {"owner":"effective-uid", "requireRegularFile":true, "links":1,
                "followFinalSymlink":false, "permissionMask":511, "nonblocking":true},
            "documentInput": "closed-object-or-full-ordered-sequence",
            "qualification": "independent-witness-required",
            "policy": self,
        })
    }

    pub(super) fn arguments(&self) -> [(String, RequiredArgument); 2] {
        [
            (self.state_argument.clone(), RequiredArgument::Path),
            (self.profile_argument.clone(), RequiredArgument::String),
        ]
    }

    pub(super) fn validate(&self, socket: &str) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        iri(&self.id, &mut ids)?;
        if !arg_id(&self.state_argument)
            || !arg_id(&self.profile_argument)
            || !unique_keys([socket, &self.state_argument, &self.profile_argument].into_iter())
            || !component(&self.socket_leaf)
            || self.socket_maximum_exclusive == 0
            || self.state_sources.is_empty()
            || self.runtime.is_empty()
            || self.private_directory_mode > 0o777
            || self.private_file_mode > 0o777
            || self.profile_atom.maximum_bytes == 0
            || !self
                .profile_atom
                .punctuation
                .bytes()
                .all(|byte| byte.is_ascii_punctuation() && byte != b'/')
            || !component(&self.default_profile)
            || self.default_profile.len() > usize::from(self.profile_atom.maximum_bytes)
            || !self.default_profile.bytes().all(|b| {
                b.is_ascii_alphanumeric() || self.profile_atom.punctuation.as_bytes().contains(&b)
            })
        {
            return Err("invalid local endpoint discovery policy".into());
        }
        for source in &self.state_sources {
            if source.variable.is_empty()
                || !source
                    .variable
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || !source.suffix.iter().all(|part| component(part))
            {
                return Err("invalid state environment source".into());
            }
        }
        for diagnostic in [
            &self.missing_state,
            &self.profile_atom.invalid,
            &self.unsafe_directory,
            &self.unsafe_file,
            &self.invalid_runtime_component,
            &self.socket_too_long,
        ] {
            diagnostic.validate()?;
        }
        self.profile.validate(&mut ids)?;
        self.binding.validate(&mut ids)?;
        for (doc, binding) in [(&self.profile, false), (&self.binding, true)] {
            for check in &doc.checks {
                if let Some(predicate) = &check.when {
                    self.predicate_type(predicate, binding)?;
                }
                self.predicate_type(&check.require, binding)?;
            }
        }
        for part in &self.runtime {
            match part {
                RuntimePart::Literal { value } if !value.contains(['/', '\0']) => (),
                RuntimePart::EffectiveUid => (),
                RuntimePart::ProfileField { id } if self.profile.field(id)?.ty.is_text() => (),
                _ => return Err("invalid endpoint runtime component expression".into()),
            }
        }
        Ok(())
    }

    fn operand_type(&self, value: &Operand, binding: bool) -> Result<AtomType, String> {
        Ok(match value {
            Operand::ProfileField { id } => self.profile.field(id)?.ty.atom(),
            Operand::BindingField { id } if binding => self.binding.field(id)?.ty.atom(),
            Operand::BindingField { .. } => {
                return Err("runtime binding field used before reading it".into());
            }
            Operand::ProfileName | Operand::Text { .. } => AtomType::Text,
            Operand::ProfilePath => AtomType::Path,
            Operand::UnsignedByte { .. } => AtomType::Unsigned,
            Operand::Null => AtomType::Null,
        })
    }

    fn predicate_type(&self, predicate: &Predicate, binding: bool) -> Result<(), String> {
        use AtomType::*;
        let valid = match predicate {
            Predicate::Equal { left, right } | Predicate::Different { left, right } => {
                let left = self.operand_type(left, binding)?;
                let right = self.operand_type(right, binding)?;
                left == right
                    || matches!(
                        (left, right),
                        (Path, Text)
                            | (Text, Path)
                            | (Nullable, Text | Null)
                            | (Text | Null, Nullable)
                    )
            }
            Predicate::LowerHex {
                value,
                bytes,
                nullable,
            } => {
                (1..=64).contains(bytes)
                    && self.operand_type(value, binding)? == if *nullable { Nullable } else { Text }
            }
        };
        if !valid {
            return Err("mistyped endpoint document predicate".into());
        }
        Ok(())
    }

    pub(super) fn render(&self, out: &mut String) {
        let diagnostic = super::rust::failure;
        out.push_str(include_str!("discovery_runtime.rs.in"));
        writeln!(out, "
fn discover_endpoint(matches: &ArgMatches, explicit: Option<&PathBuf>) -> Result<PathBuf, Failure> {{
    let state = matches.try_get_one::<PathBuf>({:?}).map_err(|_| BINDING)?.cloned();
    let state = match state {{
        Some(path) => path,
        None => {{
            let sources: &[(&str, bool, bool, &[&str])] = &[", self.state_argument).unwrap();
        for source in &self.state_sources {
            let platform = match source.platform {
                Some(Platform::Linux) => "cfg!(target_os = \"linux\")",
                Some(Platform::Macos) => "cfg!(target_os = \"macos\")",
                None => "true",
            };
            writeln!(
                out,
                "                ({:?}, {platform}, {}, &{:?}),",
                source.variable, source.require_absolute, source.suffix
            )
            .unwrap();
        }
        writeln!(out, "
            ];
            sources.iter().find_map(|(variable, platform, absolute, suffix)| {{
                if !platform {{ return None; }}
                let base = PathBuf::from(::std::env::var_os(variable)?);
                if *absolute && !base.is_absolute() {{ return None; }}
                Some(suffix.iter().fold(base, |path, part| path.join(part)))
            }}).ok_or({})?
        }}
    }};
    if let Some(path) = explicit {{ return Ok(path.clone()); }}
    let profile_name = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str).unwrap_or({:?});
    if !endpoint_component(profile_name) || profile_name.len() > {} || !profile_name.bytes().all(|b| b.is_ascii_alphanumeric() || {:?}.as_bytes().contains(&b)) {{
        return Err({});
    }}
    let uid = unsafe {{ libc::geteuid() }};
    endpoint_private_dir(&state, uid, {}, {})?;
    let profile_path = state.canonicalize().map_err(|_| IO)?.join(profile_name);
    endpoint_private_dir(&profile_path, uid, {}, {})?;
", diagnostic(&self.missing_state), self.profile_argument, self.default_profile, self.profile_atom.maximum_bytes,
        self.profile_atom.punctuation, diagnostic(&self.profile_atom.invalid), self.private_directory_mode,
        diagnostic(&self.unsafe_directory), self.private_directory_mode, diagnostic(&self.unsafe_directory)).unwrap();
        self.render_document(out, "_endpoint_profile", "profile_path", &self.profile);
        out.push_str("    let mut runtime_name = String::new();\n");
        for part in &self.runtime {
            let expr = match part {
                RuntimePart::Literal { value } => format!("{value:?}"),
                RuntimePart::EffectiveUid => "&uid.to_string()".into(),
                RuntimePart::ProfileField { id } => format!(
                    "endpoint_text(&_endpoint_profile, {:?})",
                    self.profile.field(id).unwrap().key
                ),
            };
            writeln!(out, "    runtime_name.push_str({expr});").unwrap();
        }
        writeln!(
            out,
            "
    if !endpoint_component(&runtime_name) {{ return Err({}); }}
    let runtime = ::std::env::temp_dir().join(runtime_name);
    endpoint_private_dir(&runtime, uid, {}, {})?;
",
            diagnostic(&self.invalid_runtime_component),
            self.private_directory_mode,
            diagnostic(&self.unsafe_directory)
        )
        .unwrap();
        self.render_document(out, "_endpoint_binding", "runtime", &self.binding);
        writeln!(
            out,
            "
    let socket = runtime.join({:?});
    if socket.as_os_str().as_encoded_bytes().len() >= {} {{ return Err({}); }}
    Ok(socket)
}}",
            self.socket_leaf,
            self.socket_maximum_exclusive,
            diagnostic(&self.socket_too_long)
        )
        .unwrap();
    }

    fn render_document(&self, out: &mut String, name: &str, dir: &str, doc: &Document) {
        let diagnostic = super::rust::failure;
        writeln!(
            out,
            "    let {name} = endpoint_document(&{dir}.join({:?}), uid, {}, {}, {}, {}, {}, {}, &[",
            doc.leaf,
            self.private_file_mode,
            doc.maximum_bytes,
            matches!(doc.overflow, Overflow::Reject),
            doc.missing
                .as_ref()
                .map_or_else(|| "IO".to_owned(), diagnostic),
            diagnostic(&self.unsafe_file),
            diagnostic(&doc.invalid)
        )
        .unwrap();
        for field in &doc.fields {
            let shape = match &field.ty {
                FieldType::Text => "EndpointField::Text".to_owned(),
                FieldType::NullableText => "EndpointField::NullableText".to_owned(),
                FieldType::UnsignedByte => "EndpointField::UnsignedByte".to_owned(),
                FieldType::Strings { values } => format!("EndpointField::Strings(&{values:?})"),
            };
            writeln!(out, "        ({:?}, {shape}),", field.key).unwrap();
        }
        out.push_str("    ])?;\n");
        for check in &doc.checks {
            let when = check.when.as_ref().map_or_else(String::new, |predicate| {
                format!("({}) && ", self.condition(predicate))
            });
            writeln!(
                out,
                "    if {when}!({}) {{ return Err({}); }}",
                self.condition(&check.require),
                diagnostic(&check.rejected)
            )
            .unwrap();
        }
    }

    fn value(&self, operand: &Operand) -> String {
        match operand {
            Operand::ProfileField { id } => format!(
                "endpoint_value(&_endpoint_profile, {:?})",
                self.profile.field(id).unwrap().key
            ),
            Operand::BindingField { id } => format!(
                "endpoint_value(&_endpoint_binding, {:?})",
                self.binding.field(id).unwrap().key
            ),
            Operand::ProfileName => "EndpointValue::Text(profile_name)".into(),
            Operand::ProfilePath => "EndpointValue::Path(&profile_path)".into(),
            Operand::Text { value } => format!("EndpointValue::Text({value:?})"),
            Operand::UnsignedByte { value } => format!("EndpointValue::UnsignedByte({value})"),
            Operand::Null => "EndpointValue::Null".into(),
        }
    }

    fn condition(&self, predicate: &Predicate) -> String {
        match predicate {
            Predicate::Equal { left, right } | Predicate::Different { left, right } => {
                let negate = if matches!(predicate, Predicate::Different { .. }) {
                    "!"
                } else {
                    ""
                };
                format!(
                    "{negate}endpoint_equal({}, {})",
                    self.value(left),
                    self.value(right)
                )
            }
            Predicate::LowerHex {
                value,
                bytes,
                nullable,
            } => format!(
                "endpoint_lower_hex({}, {}, {nullable})",
                self.value(value),
                u16::from(*bytes) * 2
            ),
        }
    }
}

impl Document {
    fn field(&self, id: &str) -> Result<&Field, String> {
        self.fields
            .iter()
            .find(|field| field.id == id)
            .ok_or_else(|| format!("unknown endpoint document field {id}"))
    }

    fn validate(&self, ids: &mut BTreeSet<String>) -> Result<(), String> {
        iri(&self.id, ids)?;
        self.invalid.validate()?;
        if let Some(missing) = &self.missing {
            missing.validate()?;
        }
        if !component(&self.leaf)
            || !(1..=16_777_216).contains(&self.maximum_bytes)
            || self.fields.is_empty()
            || !unique_keys(self.fields.iter().map(|field| field.key.as_str()))
        {
            return Err("invalid endpoint document schema".into());
        }
        for field in &self.fields {
            iri(&field.id, ids)?;
            if let FieldType::Strings { values } = &field.ty {
                if values.is_empty() || !unique_keys(values.iter().map(String::as_str)) {
                    return Err("invalid endpoint document enum".into());
                }
            }
        }
        for check in &self.checks {
            iri(&check.id, ids)?;
            check.rejected.validate()?;
        }
        Ok(())
    }
}

impl FieldType {
    fn atom(&self) -> AtomType {
        match self {
            Self::Text | Self::Strings { .. } => AtomType::Text,
            Self::NullableText => AtomType::Nullable,
            Self::UnsignedByte => AtomType::Unsigned,
        }
    }
    fn is_text(&self) -> bool {
        self.atom() == AtomType::Text
    }
}
