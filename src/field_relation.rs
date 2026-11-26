//! Typed comparisons and ordered preflight calls on projected native record types.
use crate::{
    shared_declarations::unique_map,
    wire_schema::{digest, encode, iri, json, symbol, type_symbol},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-field-relation-v1";
const RECEIVER: &str = "receiver";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, Type>,
    #[serde(deserialize_with = "unique_map")]
    calls: BTreeMap<String, Signature>,
    #[serde(deserialize_with = "unique_map")]
    relations: BTreeMap<String, Relation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Type {
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    equality: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    copy: bool,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Value {
    Bool {},
    Unsigned { bits: u8 },
    FixedBytes { size: u32 },
    Bytes {},
    Text {},
    Named { id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    receiver: String,
    arguments: Vec<Value>,
    output: Option<Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Relation {
    receiver: String,
    arguments: Vec<Argument>,
    steps: Vec<Step>,
    failure: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Argument {
    id: String,
    value: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Step {
    Equal { left: Operand, right: Operand },
    Check { call: Call },
}

#[derive(PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Operand {
    Field { object: String, field: String },
    Argument { id: String },
    Result { call: Call },
}

#[derive(PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Call {
    method: String,
    object: String,
    arguments: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    calls: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    relations: BTreeMap<String, Method>,
    error_type: Option<String>,
    #[serde(deserialize_with = "unique_map")]
    errors: BTreeMap<String, Fault>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Method {
    name: String,
    #[serde(deserialize_with = "unique_map")]
    arguments: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fault {
    variant: String,
    argument: Option<String>,
}

fn native_path(path: &str) -> bool {
    path.strip_prefix("crate::").is_some_and(|path| {
        path.split("::").all(symbol) && path.rsplit("::").next().is_some_and(type_symbol)
    })
}

fn variable(name: &str) -> bool {
    symbol(name) && name.as_bytes()[0].is_ascii_lowercase()
}

fn declare<'a>(ids: &mut BTreeSet<&'a str>, id: &'a str) -> Result<()> {
    iri(id)?;
    if !ids.insert(id) {
        return Err(format!("duplicate field relation identity: {id}"));
    }
    Ok(())
}

impl Graph {
    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.format != FORMAT
            || binding.format != "oddlang-field-relation-rust-v1"
            || self.relations.is_empty()
            || !self.types.keys().eq(binding.types.keys())
            || !self.relations.keys().eq(binding.relations.keys())
            || !self.calls.keys().eq(binding.calls.keys())
        {
            return Err("field relations need known format and exact bindings".into());
        }
        iri(&self.id)?;
        let mut ids = BTreeSet::from([self.id.as_str()]);
        let mut native_types = BTreeSet::new();
        let mut fields = BTreeSet::new();
        for (id, record) in &self.types {
            declare(&mut ids, id)?;
            let path = &binding.types[id];
            if !native_path(path) || !native_types.insert(path) {
                return Err(format!("invalid or duplicate relation type path: {id}"));
            }
            let mut native_fields = BTreeSet::new();
            for (id, value) in &record.fields {
                declare(&mut ids, id)?;
                fields.insert(id);
                self.check_value(value)?;
                if !binding
                    .fields
                    .get(id)
                    .is_some_and(|field| variable(field) && native_fields.insert(field))
                {
                    return Err(format!("invalid or duplicate native relation field: {id}"));
                }
            }
        }
        if !fields.into_iter().eq(binding.fields.keys()) {
            return Err("field relations need exact native field bindings".into());
        }
        let mut methods = BTreeSet::new();
        for (id, signature) in &self.calls {
            declare(&mut ids, id)?;
            let method = &binding.calls[id];
            if !self.types.contains_key(&signature.receiver)
                || !symbol(method)
                || !methods.insert((&signature.receiver, method))
            {
                return Err(format!("invalid or duplicate native relation call: {id}"));
            }
            for value in signature.arguments.iter().chain(signature.output.iter()) {
                self.check_value(value)?;
            }
        }
        let mut errors = BTreeSet::new();
        for (id, relation) in &self.relations {
            declare(&mut ids, id)?;
            let method = &binding.relations[id];
            if !self.types.contains_key(&relation.receiver)
                || !symbol(&method.name)
                || !methods.insert((&relation.receiver, &method.name))
                || relation.steps.is_empty()
                || !relation
                    .steps
                    .iter()
                    .any(|step| matches!(step, Step::Equal { .. }))
            {
                return Err(format!("invalid or duplicate native relation method: {id}"));
            }
            let mut native_args = BTreeSet::new();
            let mut args = BTreeMap::new();
            for argument in &relation.arguments {
                declare(&mut ids, &argument.id)?;
                self.check_value(&argument.value)?;
                args.insert(argument.id.as_str(), &argument.value);
                if !method
                    .arguments
                    .get(&argument.id)
                    .is_some_and(|name| variable(name) && native_args.insert(name))
                {
                    return Err(format!(
                        "invalid relation argument binding: {}",
                        argument.id
                    ));
                }
            }
            if !args
                .keys()
                .copied()
                .eq(method.arguments.keys().map(String::as_str))
            {
                return Err(format!(
                    "relation does not have exact argument bindings: {id}"
                ));
            }
            if let Some(error) = &relation.failure {
                errors.insert(error);
            }
            let used = std::cell::RefCell::new(BTreeSet::new());
            let mark = |name: &str| {
                if let Some(&id) = args.keys().find(|&&id| id == name) {
                    used.borrow_mut().insert(id);
                }
            };
            let object = |name: &str| {
                mark(name);
                if name == RECEIVER {
                    Some(relation.receiver.as_str())
                } else if let Some(Value::Named { id }) = args.get(name).copied() {
                    Some(id.as_str())
                } else {
                    None
                }
            };
            let call_type = |call: &Call| -> Result<Option<&Value>> {
                let signature = self
                    .calls
                    .get(&call.method)
                    .ok_or_else(|| format!("unknown relation call: {}", call.method))?;
                if relation.failure.is_none()
                    || object(&call.object) != Some(&signature.receiver)
                    || !call
                        .arguments
                        .iter()
                        .map(|argument| args.get(argument.as_str()).copied())
                        .eq(signature.arguments.iter().map(Some))
                {
                    return Err(format!("ill-typed fallible relation call: {}", call.method));
                }
                for argument in &call.arguments {
                    mark(argument);
                    if let Some(Value::Named { id }) = args.get(argument.as_str()).copied() {
                        if self.types[id].fields.is_empty() && !self.types[id].copy {
                            return Err(format!(
                                "opaque relation call argument requires Copy: {argument}"
                            ));
                        }
                    }
                }
                Ok(signature.output.as_ref())
            };
            let operand_type = |operand: &Operand| -> Result<&Value> {
                match operand {
                    Operand::Field {
                        object: owner,
                        field,
                    } => object(owner).and_then(|id| self.types[id].fields.get(field)),
                    Operand::Argument { id } => {
                        mark(id);
                        args.get(id.as_str()).copied()
                    }
                    Operand::Result { call } => call_type(call)?,
                }
                .ok_or_else(|| {
                    "unknown field, argument or unit result in relation comparison".into()
                })
            };
            for step in &relation.steps {
                match step {
                    Step::Equal { left, right } => {
                        let left_type = operand_type(left)?;
                        let right_type = operand_type(right)?;
                        if left_type != right_type
                            || matches!(left_type, Value::Named { id } if !self.types[id].equality)
                        {
                            return Err(format!("incompatible types in relation comparison: {id}"));
                        }
                        if left == right && !matches!(left, Operand::Result { .. }) {
                            return Err(format!("reflexive comparison in relation: {id}"));
                        }
                    }
                    Step::Check { call } => {
                        if call_type(call)?.is_some() {
                            return Err(format!("relation preflight must return unit: {id}"));
                        }
                    }
                }
            }
            if !used.borrow().iter().copied().eq(args.keys().copied()) {
                return Err(format!("unused argument in field relation: {id}"));
            }
        }
        if !errors.iter().copied().eq(binding.errors.keys())
            || (errors.is_empty() && binding.error_type.is_some())
            || (!errors.is_empty() && !binding.error_type.as_deref().is_some_and(native_path))
        {
            return Err("field relations need exact typed failure bindings".into());
        }
        for error in errors {
            declare(&mut ids, error)?;
            if !type_symbol(&binding.errors[error].variant) {
                return Err(format!("invalid relation error variant: {error}"));
            }
        }
        Ok(())
    }

    fn check_value(&self, value: &Value) -> Result<()> {
        match value {
            Value::Unsigned {
                bits: 8 | 16 | 32 | 64,
            }
            | Value::Bool {}
            | Value::Bytes {}
            | Value::Text {} => Ok(()),
            Value::FixedBytes { size } if *size != 0 => Ok(()),
            Value::Named { id } if self.types.contains_key(id) => Ok(()),
            _ => Err("invalid type in field relation".into()),
        }
    }

    fn type_text(&self, value: &Value, binding: &Binding, argument: bool) -> String {
        match value {
            Value::Bool {} => "::core::primitive::bool".into(),
            Value::Unsigned { bits } => format!("::core::primitive::u{bits}"),
            Value::FixedBytes { size } => format!("[::core::primitive::u8; {size}]"),
            Value::Bytes {} if argument => "&[::core::primitive::u8]".into(),
            Value::Bytes {} => "::std::vec::Vec<::core::primitive::u8>".into(),
            Value::Text {} if argument => "&::core::primitive::str".into(),
            Value::Text {} => "::std::string::String".into(),
            Value::Named { id } if argument && !self.types[id].fields.is_empty() => {
                format!("&{}", binding.types[id])
            }
            Value::Named { id } => binding.types[id].clone(),
        }
    }

    fn borrow(&self, value: &Value, expression: &str, argument: bool) -> String {
        match value {
            Value::Bytes {} if !argument => format!("({expression}).as_slice()"),
            Value::Text {} if !argument => format!("({expression}).as_str()"),
            Value::Bytes {} | Value::Text {} => expression.into(),
            Value::Named { id } if argument && !self.types[id].fields.is_empty() => {
                expression.into()
            }
            _ => format!("&({expression})"),
        }
    }

    fn render(&self, binding: &Binding) -> String {
        let mut source = String::new();
        for (id, relation) in &self.relations {
            let native = &binding.relations[id];
            writeln!(source, "impl {} {{", binding.types[&relation.receiver]).unwrap();
            write!(source, "    pub fn {}(&self", native.name).unwrap();
            for argument in &relation.arguments {
                write!(
                    source,
                    ", {}: {}",
                    native.arguments[&argument.id],
                    self.type_text(&argument.value, binding, true)
                )
                .unwrap();
            }
            let failure = relation.failure.as_ref().map(|failure| {
                let ty = binding
                    .error_type
                    .as_deref()
                    .expect("validated error binding");
                let fault = &binding.errors[failure];
                let constructor = format!("{ty}::{}", fault.variant);
                let error = fault
                    .argument
                    .as_ref()
                    .map_or(constructor.clone(), |value| {
                        format!("{constructor}({value:?})")
                    });
                (ty, error)
            });
            if let Some((ty, _)) = &failure {
                writeln!(source, ") -> ::core::result::Result<(), {ty}> {{").unwrap();
            } else {
                writeln!(source, ") -> ::core::primitive::bool {{").unwrap();
            };
            let object = |id: &str| {
                if id == RECEIVER {
                    "self"
                } else {
                    &native.arguments[id]
                }
            };
            let args: BTreeMap<_, _> = relation
                .arguments
                .iter()
                .map(|arg| (arg.id.as_str(), &arg.value))
                .collect();
            let call = |call: &Call| {
                let args = call
                    .arguments
                    .iter()
                    .map(|id| {
                        let argument = &native.arguments[id];
                        if matches!(args[id.as_str()], Value::Named { id } if self.types[id].fields.is_empty()) {
                            format!("*(&{argument})")
                        } else {
                            argument.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                let error = binding
                    .error_type
                    .as_deref()
                    .expect("checked call has a native fault type");
                let output = self.calls[&call.method].output.as_ref().map_or_else(
                    || "()".into(),
                    |value| self.type_text(value, binding, false),
                );
                format!(
                    "{{ let __outcome: ::core::result::Result<{output}, {error}> = {}.{}({}); __outcome? }}",
                    object(&call.object),
                    binding.calls[&call.method],
                    args.join(", ")
                )
            };
            let operand = |operand: &Operand| match operand {
                Operand::Field {
                    object: owner,
                    field,
                } => {
                    let ty = if owner == RECEIVER {
                        &relation.receiver
                    } else if let Value::Named { id } = args[owner.as_str()] {
                        id
                    } else {
                        unreachable!("validated object reference")
                    };
                    self.borrow(
                        &self.types[ty].fields[field],
                        &format!("{}.{}", object(owner), binding.fields[field]),
                        false,
                    )
                }
                Operand::Argument { id } => {
                    self.borrow(args[id.as_str()], &native.arguments[id], true)
                }
                Operand::Result { call: invocation } => self.borrow(
                    self.calls[&invocation.method]
                        .output
                        .as_ref()
                        .expect("validated outcome"),
                    &call(invocation),
                    false,
                ),
            };
            for step in &relation.steps {
                match step {
                    Step::Check { call: invocation } => {
                        writeln!(source, "        {};", call(invocation)).unwrap()
                    }
                    Step::Equal { left, right } => {
                        let on_mismatch = match &failure {
                            Some((_, error)) => format!("::core::result::Result::Err({error})"),
                            None => "false".into(),
                        };
                        writeln!(
                            source,
                            "        if {} != {} {{ return {on_mismatch}; }}",
                            operand(left),
                            operand(right)
                        )
                        .unwrap();
                    }
                }
            }
            let success = if failure.is_some() {
                "::core::result::Result::Ok(())"
            } else {
                "true"
            };
            writeln!(source, "        {success}\n    }}\n}}\n").unwrap();
        }
        source
    }
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<()> {
    let graph: Graph = json(graph)?;
    let binding: Binding = json(binding)?;
    graph.validate(&binding)?;
    let source = graph.render(&binding);
    let obligations = encode(&json!({
        "domain":graph.id,
        "obligations":graph.relations.iter().map(|(id,relation)| json!({
            "declaration":id,"role":"field-relation","expected":relation,"qualification":"independent-witness-required"
        })).collect::<Vec<_>>()
    }))?;
    let lock = encode(&json!({
        "format":FORMAT,"graphSha256":digest(&encode(&graph)?),"bindingSha256":digest(&encode(&binding)?),
        "relationSha256":digest(source.as_bytes()),"obligationsSha256":digest(&obligations),
    }))?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, bytes) in [
        ("relations.rs", source.as_bytes()),
        ("obligations.json", &obligations),
        ("projection.lock", &lock),
    ] {
        fs::write(stage.path().join(name), bytes).map_err(|e| e.to_string())?;
    }
    if output.symlink_metadata().is_ok() {
        return Err("field relation destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|e| e.to_string())
}
