//! Typed guard compilation. Graphs select only closed expressions and declared native capabilities.
use super::{EndpointView, Native, Protocol, rust};
use crate::wire_schema::{Binding, Definition, Field, Payload, Result, Schema, Shape, iri, symbol};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordCheck {
    pub owner: String,
    pub steps: Vec<Step>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Foreign {
    pub id: String,
    pub fields: Vec<Field>,
    pub variants: Vec<String>,
    pub methods: Vec<Method>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Method {
    pub id: String,
    pub returns: ReturnKind,
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReturnKind {
    Unit,
    Unsigned,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Path {
    scope: String,
    fields: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Number {
    Constant {
        value: u64,
    },
    Field {
        value: Path,
    },
    Length {
        value: Path,
    },
    JsonStringLength {
        value: Path,
    },
    SaturatingAdd {
        left: Box<Number>,
        right: Box<Number>,
    },
    SaturatingSubtract {
        left: Box<Number>,
        right: Box<Number>,
    },
    BigEndianPrefix {
        value: Path,
        bytes: usize,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Comparison {
    Equal,
    NotEqual,
    Less,
    AtMost,
    Greater,
    AtLeast,
}

impl Comparison {
    fn operator(&self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::AtMost => "<=",
            Self::Greater => ">",
            Self::AtLeast => ">=",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Condition {
    Compare {
        left: Number,
        operator: Comparison,
        right: Number,
    },
    Equal {
        left: Path,
        right: Path,
    },
    Bool {
        value: Path,
    },
    Present {
        value: Path,
    },
    Unique {
        value: Path,
    },
    ElementAbsent {
        value: Path,
        element: u64,
    },
    FirstBytesNonempty {
        value: Path,
    },
    BytesExclude {
        value: Path,
        byte: u8,
    },
    TotalBytes {
        value: Path,
        overhead: u32,
        maximum: u64,
    },
    TextEqual {
        value: Path,
        text: String,
    },
    TextPrefix {
        value: Path,
        text: String,
    },
    TextExcludesNul {
        value: Path,
    },
    Variant {
        value: Path,
        variant: String,
    },
    All {
        items: Vec<Condition>,
    },
    Any {
        items: Vec<Condition>,
    },
    Not {
        predicate: Box<Condition>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Step {
    CheckRecord {
        id: String,
        value: Path,
        check: String,
    },
    Require {
        id: String,
        predicate: Condition,
        error: String,
    },
    If {
        id: String,
        predicate: Condition,
        steps: Vec<Step>,
    },
    Some {
        id: String,
        value: Path,
        bind: String,
        steps: Vec<Step>,
    },
    ForEach {
        id: String,
        value: Path,
        bind: String,
        steps: Vec<Step>,
    },
    Cases {
        id: String,
        value: Path,
        arms: Vec<Arm>,
    },
    Call {
        id: String,
        value: Path,
        method: String,
    },
    OnSuccess {
        id: String,
        value: Path,
        method: String,
        bind: String,
        steps: Vec<Step>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Arm {
    variant: String,
    bind: String,
    steps: Vec<Step>,
}

#[derive(Clone)]
struct Value {
    rust: String,
    shape: Shape,
}

#[derive(Clone)]
enum Provided {
    Value(Value),
    Fields(BTreeMap<String, Value>),
}

type Scope = BTreeMap<String, Provided>;

fn native_fields(fields: &[Field], prefix: &str) -> Provided {
    Provided::Fields(
        fields
            .iter()
            .map(|field| {
                (
                    field.id.clone(),
                    Value {
                        rust: format!("&{prefix}{}", field.name),
                        shape: field.shape.clone(),
                    },
                )
            })
            .collect(),
    )
}

fn supplied(scope: &mut Scope, name: &str, value: Provided) -> Result<()> {
    if name.is_empty() || scope.insert(name.to_owned(), value).is_some() {
        return Err("empty or shadowed guard scope name".into());
    }
    Ok(())
}

pub(super) struct Compiler<'a> {
    pub graph: &'a Protocol,
    pub schema: &'a Schema,
    pub binding: &'a Binding,
    pub native: &'a Native,
    check_indices: BTreeMap<&'a str, usize>,
    cursor: usize,
}

impl<'a> Compiler<'a> {
    pub fn new(
        graph: &'a Protocol,
        schema: &'a Schema,
        binding: &'a Binding,
        native: &'a Native,
    ) -> Self {
        Self {
            graph,
            schema,
            binding,
            native,
            check_indices: graph
                .record_checks
                .keys()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect(),
            cursor: 0,
        }
    }

    pub fn validate_foreign(&self) -> Result<()> {
        let definitions: BTreeMap<_, _> =
            self.schema.types.iter().map(|ty| (ty.id(), ty)).collect();
        let mut types = BTreeSet::new();
        let mut members = BTreeSet::new();
        for f in &self.graph.foreign {
            if !matches!(
                definitions.get(f.id.as_str()),
                Some(Definition::External { .. })
            ) || !types.insert(&f.id)
            {
                return Err("foreign guard role must identify a distinct external type".into());
            }
            for field in &f.fields {
                super::super::check_shape(&field.shape, &definitions)?;
                if !members.insert(&field.id) {
                    return Err("duplicate foreign member".into());
                }
            }
            for id in f
                .variants
                .iter()
                .chain(f.methods.iter().map(|method| &method.id))
            {
                if !members.insert(id) {
                    return Err("duplicate foreign member".into());
                }
            }
        }
        if members
            .iter()
            .copied()
            .ne(self.native.foreign_members.keys())
        {
            return Err(
                "native foreign member bindings must cover the declared member roles".into(),
            );
        }
        for (id, name) in &self.native.foreign_members {
            iri(id)?;
            if !symbol(name) {
                return Err("invalid native foreign member binding".into());
            }
        }
        Ok(())
    }

    pub fn record_checks(&mut self) -> Result<String> {
        let graph = self.graph;
        let dependencies: BTreeMap<_, _> = graph
            .record_checks
            .iter()
            .map(|(id, check)| (id.as_str(), record_calls(&check.steps)))
            .collect();
        reachable_checks(&dependencies, dependencies.keys().copied())?;
        let endpoint_steps = graph
            .endpoints
            .iter()
            .flat_map(|endpoint| endpoint.steps.iter().chain(&endpoint.timed));
        let digest_steps = graph.digests.iter().flat_map(|digest| match digest {
            super::digests::Digest::Bytes { .. } => [].iter(),
            super::digests::Digest::Case { steps, .. } => steps.iter(),
        });
        let reachable = reachable_checks(
            &dependencies,
            record_calls(endpoint_steps.chain(digest_steps)),
        )?;
        let mut functions = String::new();
        for (id, check) in &graph.record_checks {
            if !matches!(self.definition(&check.owner)?, Definition::Record { .. }) {
                return Err("a shared check requires a named record".into());
            }
            let value = Provided::Value(Value {
                rust: "_record".into(),
                shape: Shape::Ref {
                    id: check.owner.clone(),
                },
            });
            let body = self.compile(value, &check.steps, false)?;
            if reachable.contains(id.as_str()) {
                let index = self.check_indices[id.as_str()];
                let native = &self.binding.names[&check.owner];
                writeln!(functions, "\nfn __oddlang_record_check_{index}(_record: &{native}) -> ::core::result::Result<(), {}> {{\n{body}    ::core::result::Result::Ok(())\n}}", rust::ERROR).unwrap();
            }
        }
        Ok(functions)
    }

    pub fn endpoint(
        &mut self,
        view: &EndpointView<'_>,
        steps: &[Step],
        clock: bool,
    ) -> Result<String> {
        let receiver = if clock { "self." } else { "value." };
        self.compile(native_fields(&view.fields, receiver), steps, clock)
    }

    pub fn digest(&mut self, fields: &[Field], steps: &[Step]) -> Result<String> {
        self.compile(native_fields(fields, ""), steps, false)
    }

    fn compile(&mut self, operands: Provided, steps: &[Step], clock: bool) -> Result<String> {
        let mut scope = Scope::from([("value".to_owned(), operands)]);
        if clock {
            scope.insert(
                "clock".to_owned(),
                Provided::Value(Value {
                    rust: "&now".to_owned(),
                    shape: Shape::Unsigned { bits: 64 },
                }),
            );
        }
        let mut result = String::new();
        self.sequence(steps, &scope, "    ", &mut result)?;
        Ok(result)
    }

    fn fresh(&mut self) -> String {
        let name = format!("__oddlang_guard_{}", self.cursor);
        self.cursor += 1;
        name
    }

    fn foreign(&self, id: &str) -> Option<&Foreign> {
        self.graph.foreign.iter().find(|entry| entry.id == id)
    }

    fn definition(&self, id: &str) -> Result<&Definition> {
        self.schema
            .types
            .iter()
            .find(|definition| definition.id() == id)
            .ok_or_else(|| format!("unknown guard type {id}"))
    }

    fn select_field(&self, value: Value, member: &str) -> Result<Value> {
        let Shape::Ref { id } = &value.shape else {
            return Err("guard field path must traverse a named record".into());
        };
        let (fields, names) = match self.definition(id)? {
            Definition::Record { fields, .. } => (fields, &self.binding.names),
            Definition::External { .. } => (
                &self
                    .foreign(id)
                    .ok_or("undeclared external guard type")?
                    .fields,
                &self.native.foreign_members,
            ),
            Definition::UnitEnum { .. } | Definition::Adjacent { .. } => {
                return Err("match the case before accessing a union field".into());
            }
        };
        let field = fields
            .iter()
            .find(|f| f.id == member)
            .ok_or("unknown guard field in record")?;
        Ok(Value {
            rust: format!("&({}).{}", value.rust, names[member]),
            shape: field.shape.clone(),
        })
    }

    fn path(&self, path: &Path, scope: &Scope) -> Result<Value> {
        let root = scope
            .get(&path.scope)
            .ok_or_else(|| format!("unknown guard scope {}", path.scope))?;
        let mut fields = path.fields.iter();
        let mut value = match root {
            Provided::Value(value) => value.clone(),
            Provided::Fields(record) => {
                let field = fields
                    .next()
                    .ok_or("guard path must select a field from its scope")?;
                record
                    .get(field)
                    .ok_or_else(|| format!("unknown guard field {field}"))?
                    .clone()
            }
        };
        for member in fields {
            value = self.select_field(value, member)?;
        }
        Ok(value)
    }

    fn number(&self, number: &Number, scope: &Scope) -> Result<String> {
        match number {
            Number::Constant { value } => Ok(format!("{value}u64")),
            Number::Field { value } => {
                let value = self.path(value, scope)?;
                if !matches!(value.shape, Shape::Unsigned { .. }) {
                    return Err("guard needs an unsigned integer".into());
                }
                Ok(format!("::core::primitive::u64::from(*({}))", value.rust))
            }
            Number::Length { value } => {
                let value = self.path(value, scope)?;
                if !matches!(
                    value.shape,
                    Shape::List { .. }
                        | Shape::BoundedBase64List { .. }
                        | Shape::BoundedBase64 { .. }
                        | Shape::LowerHex { .. }
                        | Shape::Text {}
                ) {
                    return Err("guard length needs ordered values, bytes or text".into());
                }
                Ok(format!("({}).len() as u64", value.rust))
            }
            Number::JsonStringLength { value } => {
                let value = self.text(value, scope)?;
                Ok(format!(
                    r#"({}).bytes().fold(2u64, |size, octet| size.saturating_add(match octet {{
                        b'"' | b'\\' | b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' => 2,
                        0..=0x1f => 6,
                        _ => 1,
                    }}))"#,
                    value.rust
                ))
            }
            Number::SaturatingAdd { left, right } | Number::SaturatingSubtract { left, right } => {
                let op = if matches!(number, Number::SaturatingAdd { .. }) {
                    "saturating_add"
                } else {
                    "saturating_sub"
                };
                Ok(format!(
                    "({}).{op}({})",
                    self.number(left, scope)?,
                    self.number(right, scope)?
                ))
            }
            Number::BigEndianPrefix { value, bytes } => {
                let value = self.path(value, scope)?;
                if !matches!(value.shape, Shape::LowerHex { bytes: width, .. } if (1..=8).contains(bytes) && *bytes <= width)
                {
                    return Err("big-endian prefix requires enough fixed bytes".into());
                }
                let padding = 8 - bytes;
                Ok(format!(
                    "{{ let mut raw = [0u8; 8]; raw[{padding}..].copy_from_slice(&({})[..{bytes}]); u64::from_be_bytes(raw) }}",
                    value.rust
                ))
            }
        }
    }

    fn tagged(&self, value: &Value, variant: &str) -> Result<String> {
        let Shape::Ref { id } = &value.shape else {
            return Err("variant guard requires a declared union".into());
        };
        match self.definition(id)? {
            Definition::UnitEnum { variants, .. } | Definition::Adjacent { variants, .. } => {
                let tagged = variants
                    .iter()
                    .find(|case| case.id == variant)
                    .ok_or("unrecognized guard case")?;
                let discard = match tagged.payload {
                    Payload::Unit {} => "",
                    Payload::Newtype { .. } => "(..)",
                    Payload::Record { .. } => " { .. }",
                };
                Ok(format!(
                    "matches!({}, {}::{}{discard})",
                    value.rust, self.binding.names[id], self.binding.names[variant]
                ))
            }
            Definition::External { .. } => {
                if !self
                    .foreign(id)
                    .is_some_and(|f| f.variants.iter().any(|v| v == variant))
                {
                    return Err("undeclared external wire tag".into());
                }
                Ok(format!(
                    "matches!({}, {}::{})",
                    value.rust, self.binding.externals[id], self.native.foreign_members[variant]
                ))
            }
            Definition::Record { .. } => Err("variant guard requires a union".into()),
        }
    }

    fn list_bytes(&self, value: &Value) -> Result<()> {
        if matches!(value.shape, Shape::BoundedBase64List { .. }) {
            Ok(())
        } else {
            Err("guard needs a byte sequence list".into())
        }
    }

    fn text(&self, path: &Path, scope: &Scope) -> Result<Value> {
        let value = self.path(path, scope)?;
        if !matches!(value.shape, Shape::Text {}) {
            return Err("guard expects text".into());
        }
        Ok(value)
    }

    fn condition(&self, condition: &Condition, scope: &Scope) -> Result<String> {
        let path = |p: &Path| self.path(p, scope);
        match condition {
            Condition::Compare {
                left,
                operator,
                right,
            } => Ok(format!(
                "({}) {} ({})",
                self.number(left, scope)?,
                operator.operator(),
                self.number(right, scope)?
            )),
            Condition::Equal { left, right } => {
                let left = path(left)?;
                let right = path(right)?;
                if !same_equal_shape(&left.shape, &right.shape, self.binding) {
                    return Err("guard equality needs matching comparable operands".into());
                }
                Ok(format!("({}) == ({})", left.rust, right.rust))
            }
            Condition::Bool { value } => {
                let value = path(value)?;
                if !matches!(value.shape, Shape::Bool {}) {
                    return Err("guard expects a boolean".into());
                }
                Ok(format!("*({})", value.rust))
            }
            Condition::Present { value } => {
                let value = path(value)?;
                if !matches!(value.shape, Shape::Optional { .. }) {
                    return Err("presence guard expects an optional value".into());
                }
                Ok(format!("({}).is_some()", value.rust))
            }
            Condition::Unique { value } => {
                let value = path(value)?;
                match &value.shape {
                    Shape::List { item }
                        if crate::wire_schema::supports_derive(item, "PartialEq", self.binding) => {
                    }
                    Shape::BoundedBase64List { .. } => {}
                    _ => return Err("uniqueness requires comparable ordered elements".into()),
                }
                Ok(format!(
                    "{{ let ordered = {}; ordered.iter().enumerate().all(|(pos, item)| !ordered[..pos].contains(item)) }}",
                    value.rust
                ))
            }
            Condition::ElementAbsent { value, element } => {
                let value = path(value)?;
                if !matches!(&value.shape, Shape::List { item } if matches!(item.as_ref(), Shape::Unsigned { .. }))
                {
                    return Err("unsigned membership expects an unsigned list".into());
                }
                Ok(format!(
                    "({}).iter().all(|item| u64::from(*item) != {element}u64)",
                    value.rust
                ))
            }
            Condition::FirstBytesNonempty { value } => {
                let value = path(value)?;
                self.list_bytes(&value)?;
                Ok(format!(
                    "({}).first().is_some_and(|item| !item.is_empty())",
                    value.rust
                ))
            }
            Condition::BytesExclude { value, byte } => {
                let value = path(value)?;
                self.list_bytes(&value)?;
                Ok(format!(
                    "({}).iter().all(|item| !item.contains(&{byte}))",
                    value.rust
                ))
            }
            Condition::TotalBytes {
                value,
                overhead,
                maximum,
            } => {
                let value = path(value)?;
                self.list_bytes(&value)?;
                Ok(format!(
                    "({}).iter().try_fold(0u64, |total, item| total.checked_add(item.len() as u64)?.checked_add({overhead})).is_some_and(|total| total <= {maximum}u64)",
                    value.rust
                ))
            }
            Condition::TextEqual { value, text } => {
                let value = self.text(value, scope)?;
                Ok(format!("({}).as_str() == {text:?}", value.rust))
            }
            Condition::TextPrefix { value, text } => {
                let value = self.text(value, scope)?;
                Ok(format!("({}).starts_with({text:?})", value.rust))
            }
            Condition::TextExcludesNul { value } => {
                let value = self.text(value, scope)?;
                Ok(format!("!({}).as_bytes().contains(&0)", value.rust))
            }
            Condition::Variant { value, variant } => self.tagged(&path(value)?, variant),
            Condition::All { items } | Condition::Any { items } => {
                if items.is_empty() {
                    return Err("a boolean combination needs a predicate".into());
                }
                let op = if matches!(condition, Condition::All { .. }) {
                    " && "
                } else {
                    " || "
                };
                let parts = items
                    .iter()
                    .map(|item| self.condition(item, scope).map(|part| format!("({part})")))
                    .collect::<Result<Vec<_>>>()?;
                Ok(parts.join(op))
            }
            Condition::Not { predicate } => Ok(format!("!({})", self.condition(predicate, scope)?)),
        }
    }

    fn method(&self, value: &Value, method: &str, returns: ReturnKind) -> Result<String> {
        let Shape::Ref { id } = &value.shape else {
            return Err("a guard call needs a declared external receiver".into());
        };
        let known = self.foreign(id).ok_or("unbound external guard receiver")?;
        if !known
            .methods
            .iter()
            .any(|item| item.id == method && item.returns == returns)
        {
            return Err("unknown or mistyped external operation".into());
        }
        Ok(format!(
            "({}).{}()",
            value.rust, self.native.foreign_members[method]
        ))
    }

    fn sequence(
        &mut self,
        steps: &[Step],
        scope: &Scope,
        indent: &str,
        text: &mut String,
    ) -> Result<()> {
        let child = format!("{indent}    ");
        for step in steps {
            match step {
                Step::CheckRecord { value, check, .. } => {
                    let value = self.path(value, scope)?;
                    let spec = self
                        .graph
                        .record_checks
                        .get(check)
                        .ok_or("undeclared record check")?;
                    let index = self.check_indices[check.as_str()];
                    if !matches!(&value.shape, Shape::Ref { id } if id == &spec.owner) {
                        return Err("record check requires its declared type".into());
                    }
                    writeln!(
                        text,
                        "{indent}__oddlang_record_check_{index}({})?;",
                        value.rust
                    )
                    .unwrap();
                }
                Step::Require {
                    predicate, error, ..
                } => {
                    let condition = self.condition(predicate, scope)?;
                    if !self.graph.errors.contains_key(error) {
                        return Err("unbound typed guard error".into());
                    }
                    let error = rust::error(self.graph, self.native, error);
                    writeln!(text, "{indent}if !({condition}) {{\n{child}return ::core::result::Result::Err({error});\n{indent}}}").unwrap();
                }
                Step::If {
                    predicate, steps, ..
                } => {
                    let condition = self.condition(predicate, scope)?;
                    writeln!(text, "{indent}if {condition} {{").unwrap();
                    self.sequence(steps, scope, &child, text)?;
                    writeln!(text, "{indent}}}").unwrap();
                }
                Step::Some {
                    value, bind, steps, ..
                } => {
                    let value = self.path(value, scope)?;
                    let Shape::Optional { item } = value.shape else {
                        return Err("optional guard must bind an optional value".into());
                    };
                    let rust = self.fresh();
                    writeln!(
                        text,
                        "{indent}if let ::core::option::Option::Some({rust}) = ({}).as_ref() {{",
                        value.rust
                    )
                    .unwrap();
                    let mut scope = scope.clone();
                    supplied(
                        &mut scope,
                        bind,
                        Provided::Value(Value { rust, shape: *item }),
                    )?;
                    self.sequence(steps, &scope, &child, text)?;
                    writeln!(text, "{indent}}}").unwrap();
                }
                Step::ForEach {
                    value, bind, steps, ..
                } => {
                    let value = self.path(value, scope)?;
                    let Shape::List { item } = value.shape else {
                        return Err("guard iteration requires a typed list".into());
                    };
                    let rust = self.fresh();
                    writeln!(text, "{indent}for {rust} in ({}).iter() {{", value.rust).unwrap();
                    let mut scope = scope.clone();
                    supplied(
                        &mut scope,
                        bind,
                        Provided::Value(Value { rust, shape: *item }),
                    )?;
                    self.sequence(steps, &scope, &child, text)?;
                    writeln!(text, "{indent}}}").unwrap();
                }
                Step::Call { value, method, .. } => {
                    let call = self.method(&self.path(value, scope)?, method, ReturnKind::Unit)?;
                    writeln!(text, "{indent}{call}?;").unwrap();
                }
                Step::OnSuccess {
                    value,
                    method,
                    bind,
                    steps,
                    ..
                } => {
                    let call =
                        self.method(&self.path(value, scope)?, method, ReturnKind::Unsigned)?;
                    let name = self.fresh();
                    writeln!(
                        text,
                        "{indent}if let ::core::result::Result::Ok({name}) = {call} {{"
                    )
                    .unwrap();
                    let mut scope = scope.clone();
                    supplied(
                        &mut scope,
                        bind,
                        Provided::Value(Value {
                            rust: format!("&{name}"),
                            shape: Shape::Unsigned { bits: 64 },
                        }),
                    )?;
                    self.sequence(steps, &scope, &child, text)?;
                    writeln!(text, "{indent}}}").unwrap();
                }
                Step::Cases { value, arms, .. } => {
                    let value = self.path(value, scope)?;
                    self.cases(&value, arms, scope, indent, text)?;
                }
            }
        }
        Ok(())
    }

    fn cases(
        &mut self,
        value: &Value,
        arms: &[Arm],
        scope: &Scope,
        indent: &str,
        text: &mut String,
    ) -> Result<()> {
        let Shape::Ref { id } = &value.shape else {
            return Err("case guard needs an adjacent union".into());
        };
        let schema = self.schema;
        let Some(Definition::Adjacent { variants, .. }) =
            schema.types.iter().find(|ty| ty.id() == id)
        else {
            return Err("case guard needs an internal adjacent union".into());
        };
        let ty = &self.binding.names[id];
        let mut selected = BTreeSet::new();
        let inner = format!("{indent}        ");
        writeln!(text, "{indent}match {} {{", value.rust).unwrap();
        for arm in arms {
            let variant = variants
                .iter()
                .find(|variant| variant.id == arm.variant)
                .ok_or("unknown validation case")?;
            if !selected.insert(&arm.variant) {
                return Err("duplicate validation case".into());
            }
            write!(
                text,
                "{indent}    {ty}::{}",
                self.binding.names[&variant.id]
            )
            .unwrap();
            let mut scope = scope.clone();
            match &variant.payload {
                Payload::Unit {} => {}
                Payload::Newtype { shape } => {
                    let rust = self.fresh();
                    write!(text, "({rust})").unwrap();
                    supplied(
                        &mut scope,
                        &arm.bind,
                        Provided::Value(Value {
                            rust,
                            shape: shape.clone(),
                        }),
                    )?;
                }
                Payload::Record { fields } => {
                    text.push_str(" { ");
                    let mut members = BTreeMap::new();
                    for field in fields {
                        let rust = self.fresh();
                        write!(text, "{}: {rust}, ", self.binding.names[&field.id]).unwrap();
                        members.insert(
                            field.id.clone(),
                            Value {
                                rust,
                                shape: field.shape.clone(),
                            },
                        );
                    }
                    text.push('}');
                    supplied(&mut scope, &arm.bind, Provided::Fields(members))?;
                }
            }
            text.push_str(" => {\n");
            self.sequence(&arm.steps, &scope, &inner, text)?;
            writeln!(text, "{indent}    }}").unwrap();
        }
        if selected.len() != variants.len() {
            writeln!(text, "{indent}    _ => {{}}").unwrap();
        }
        writeln!(text, "{indent}}}").unwrap();
        Ok(())
    }
}

fn record_calls<'a>(steps: impl IntoIterator<Item = &'a Step>) -> BTreeSet<&'a str> {
    let mut calls = BTreeSet::new();
    for step in steps {
        step.visit(&mut |item| {
            if let Step::CheckRecord { check, .. } = item {
                calls.insert(check.as_str());
            }
        });
    }
    calls
}

fn reachable_checks<'a>(
    dependencies: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    roots: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>> {
    let mut active = BTreeSet::new();
    let mut finished = BTreeSet::new();
    let mut pending: Vec<_> = roots.into_iter().map(|id| (id, false)).collect();
    while let Some((id, exiting)) = pending.pop() {
        if exiting {
            active.remove(id);
            finished.insert(id);
        } else if !finished.contains(id) {
            let calls = dependencies.get(id).ok_or("undeclared record check")?;
            if !active.insert(id) {
                return Err("recursive shared record checks".into());
            }
            pending.push((id, true));
            pending.extend(calls.iter().rev().map(|&call| (call, false)));
        }
    }
    Ok(finished)
}

fn same_equal_shape(left: &Shape, right: &Shape, binding: &Binding) -> bool {
    match (left, right) {
        (Shape::Bool {}, Shape::Bool {}) | (Shape::Text {}, Shape::Text {}) => true,
        (Shape::Unsigned { bits: a }, Shape::Unsigned { bits: b }) => a == b,
        (Shape::LowerHex { bytes: a, .. }, Shape::LowerHex { bytes: b, .. }) => a == b,
        (Shape::Ref { id: a }, Shape::Ref { id: b }) => {
            a == b && crate::wire_schema::supports_derive(left, "PartialEq", binding)
        }
        _ => false,
    }
}

impl Step {
    pub fn id(&self) -> &str {
        match self {
            Self::CheckRecord { id, .. }
            | Self::Require { id, .. }
            | Self::If { id, .. }
            | Self::Some { id, .. }
            | Self::ForEach { id, .. }
            | Self::Cases { id, .. }
            | Self::Call { id, .. }
            | Self::OnSuccess { id, .. } => id,
        }
    }

    pub fn visit<'a>(&'a self, visit: &mut impl FnMut(&'a Step)) {
        visit(self);
        match self {
            Self::If { steps, .. }
            | Self::Some { steps, .. }
            | Self::ForEach { steps, .. }
            | Self::OnSuccess { steps, .. } => {
                for step in steps {
                    step.visit(visit);
                }
            }
            Self::Cases { arms, .. } => {
                for arm in arms {
                    for step in &arm.steps {
                        step.visit(visit);
                    }
                }
            }
            Self::Call { .. } | Self::CheckRecord { .. } | Self::Require { .. } => {}
        }
    }

    pub fn declarations(&self) -> Vec<&str> {
        let mut ids = Vec::new();
        self.visit(&mut |step| ids.push(step.id()));
        ids
    }

    pub fn errors<'a>(&'a self, errors: &mut BTreeSet<&'a str>) {
        match self {
            Self::Require { error, .. } => {
                errors.insert(error);
            }
            Self::If { steps, .. }
            | Self::Some { steps, .. }
            | Self::ForEach { steps, .. }
            | Self::OnSuccess { steps, .. } => {
                for step in steps {
                    step.errors(errors);
                }
            }
            Self::Cases { arms, .. } => {
                for arm in arms {
                    for step in &arm.steps {
                        step.errors(errors);
                    }
                }
            }
            Self::CheckRecord { .. } | Self::Call { .. } => {}
        }
    }
}
