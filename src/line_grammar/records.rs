use super::model::{Captures, Grammars, Graph, Kind, Result, unsigned_maximum};
use crate::{native_package::ident, shared_declarations::unique_map, wire_schema::iri};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecordResult {
    failure: Failure,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, Vec<Field>>,
    entry: Constructor,
    formatters: Vec<Formatter>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    id: String,
    code: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    id: String,
    r#type: FieldType,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum FieldType {
    OwnedText,
    StaticText,
    Boolean,
    Unsigned { bits: u8 },
    Bytes { length: u16 },
    UnsignedArray { bits: u8, length: u16 },
    RecordVector { record: String },
}

impl FieldType {
    fn scalar(&self, kind: Kind) -> bool {
        match (self, kind) {
            (Self::OwnedText, Kind::Text | Kind::StaticText)
            | (Self::StaticText, Kind::StaticText)
            | (Self::Boolean, Kind::Boolean) => true,
            (Self::Unsigned { bits }, Kind::Decimal(width)) => *bits == width,
            (Self::Bytes { length }, Kind::Bytes(width)) => *length == width,
            _ => false,
        }
    }

    fn rust_type(&self, overlays: &BTreeMap<String, NativeRecord>) -> String {
        match self {
            Self::OwnedText => "::std::string::String".into(),
            Self::StaticText => "&'static ::core::primitive::str".into(),
            Self::Boolean => "::core::primitive::bool".into(),
            Self::Unsigned { bits } => format!("::core::primitive::u{bits}"),
            Self::Bytes { length } => format!("[::core::primitive::u8; {length}]"),
            Self::UnsignedArray { bits, length } => {
                format!("[::core::primitive::u{bits}; {length}]")
            }
            Self::RecordVector { record } => format!("::std::vec::Vec<{}>", overlays[record].name),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Constructor {
    record: String,
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Value {
    Capture { id: String },
    Array { captures: Vec<String> },
    Vector { items: Vec<Constructor> },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Formatter {
    id: String,
    argument: bool,
    parts: Vec<Part>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Part {
    Literal { text: String },
    Field { field: String },
    Argument,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordBinding {
    parser: String,
    visibility: Visibility,
    pub error: NativeError,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, NativeRecord>,
    #[serde(deserialize_with = "unique_map")]
    formatters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "RecordSupport::is_empty")]
    pub support: RecordSupport,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecordSupport {
    #[serde(default, deserialize_with = "unique_map")]
    pub nested_grammars: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "unique_map")]
    pub text_choices: BTreeMap<String, String>,
}

impl RecordSupport {
    fn is_empty(&self) -> bool {
        self.nested_grammars.is_empty() && self.text_choices.is_empty()
    }
}

impl RecordBinding {
    pub(super) fn validate_support(&self, graph: &Graph, grammars: &Grammars<'_>) -> Result<()> {
        let names = self
            .support
            .nested_grammars
            .values()
            .chain(self.support.text_choices.values())
            .chain(self.records.values().map(|record| &record.name));
        if !names_are_valid(names) {
            return Err("expected distinct native support names".into());
        }
        if self
            .support
            .nested_grammars
            .keys()
            .any(|id| !grammars.contains_key(id.as_str()))
        {
            return Err("native support names an unknown nested text grammar".into());
        }
        if self
            .support
            .text_choices
            .keys()
            .any(|id| graph.text_choices(id).is_none())
        {
            return Err("native support names an unknown finite text choice capture".into());
        }
        Ok(())
    }

    pub(super) fn emit_support(&self, graph: &Graph, grammars: &Grammars<'_>, source: &mut String) {
        let visibility = self.visibility.native();
        for (id, name) in &self.support.nested_grammars {
            writeln!(
                source,
                "{visibility} fn {name}(value: &::core::primitive::str) -> ::core::primitive::bool {{ self::__grammar_{}::accepts(value) }}",
                grammars[id.as_str()]
            )
            .unwrap();
        }
        for (id, name) in &self.support.text_choices {
            let choices = graph
                .text_choices(id)
                .expect("validated finite choice capture");
            writeln!(
                source,
                "{visibility} const {name}: [&'static ::core::primitive::str; {}] = {choices:?};",
                choices.len()
            )
            .unwrap();
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeError {
    pub id: String,
    pub r#type: Vec<String>,
    pub variant: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRecord {
    name: String,
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, String>,
    derives: Vec<Trait>,
}

#[derive(Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Trait {
    Clone,
    Debug,
    PartialEq,
    Eq,
}

impl Trait {
    fn native(&self) -> &'static str {
        match self {
            Self::Clone => "::core::clone::Clone",
            Self::Debug => "::core::fmt::Debug",
            Self::PartialEq => "::core::cmp::PartialEq",
            Self::Eq => "::core::cmp::Eq",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Crate,
    Public,
}

impl Visibility {
    fn native(&self) -> &'static str {
        match self {
            Self::Crate => "pub(crate)",
            Self::Public => "pub",
        }
    }
}

fn names_are_valid<'a>(names: impl Iterator<Item = &'a String>) -> bool {
    let mut unique = BTreeSet::new();
    names
        .into_iter()
        .all(|name| ident(name) && unique.insert(name))
}

impl RecordResult {
    pub(super) fn validate_catalog_roles(
        &self,
        graph: &Graph,
        source: &RecordBinding,
        target: &crate::local_file_catalog::Record,
        names: &[String],
    ) -> Result<()> {
        let root = &self.entry.record;
        let root_binding = &source.records[root];
        let formatter = self.formatters.iter().any(|formatter| {
            formatter.argument && source.formatters.get(&formatter.id) == Some(&target.asset_name)
        });
        if root_binding.name != target.r#type || source.parser != target.parse || !formatter {
            return Err("local file catalog record type, parser or formatter differs".into());
        }
        let assets = self.records[root]
            .iter()
            .find(|field| root_binding.fields.get(&field.id) == Some(&target.assets))
            .ok_or("local file catalog asset vector is missing")?;
        let FieldType::RecordVector { record } = &assets.r#type else {
            return Err("local file catalog assets must be a typed record vector".into());
        };
        let nested = &source.records[record];
        let fields = &self.records[record];
        let lookup = |name: &str| {
            fields.iter().find(|field| {
                nested
                    .fields
                    .get(&field.id)
                    .is_some_and(|candidate| candidate == name)
            })
        };
        let Some(name_field) = lookup(&target.name) else {
            return Err("local file catalog asset name is missing".into());
        };
        if !matches!(name_field.r#type, FieldType::StaticText)
            || !lookup(&target.bytes)
                .is_some_and(|field| matches!(field.r#type, FieldType::Unsigned { bits: 64 }))
            || !lookup(&target.mode)
                .is_some_and(|field| matches!(field.r#type, FieldType::Unsigned { bits: 32 }))
            || !lookup(&target.digest)
                .is_some_and(|field| matches!(field.r#type, FieldType::Bytes { length: 32 }))
        {
            return Err("local file catalog asset field types differ".into());
        }
        let Some(Value::Vector { items }) = self.entry.fields.get(&assets.id) else {
            return Err("local file catalog asset construction order is missing".into());
        };
        let expected = items
            .iter()
            .map(|item| {
                let Some(Value::Capture { id }) = item.fields.get(&name_field.id) else {
                    return Err("local file catalog asset name must use a fixed capture");
                };
                graph
                    .rows
                    .iter()
                    .flatten()
                    .find_map(|token| match token {
                        super::model::Token::Literal {
                            text,
                            capture: Some(capture),
                        } if capture == id => Some(text.as_str()),
                        _ => None,
                    })
                    .ok_or("local file catalog asset name must use a literal capture")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !expected
            .iter()
            .copied()
            .eq(names.iter().map(String::as_str))
        {
            return Err("local file catalog asset names differ from the grammar".into());
        }
        Ok(())
    }

    pub(super) fn validate(&self, binding: &RecordBinding, captures: &Captures<'_>) -> Result<()> {
        iri(&self.failure.id)?;
        if self.failure.id != binding.error.id
            || self.failure.code.is_empty()
            || binding.error.r#type.len() < 2
            || binding.error.r#type[0] != "crate"
            || !binding.error.r#type[1..].iter().all(|part| ident(part))
            || !ident(&binding.error.variant)
            || !ident(&binding.parser)
            || !self.records.keys().eq(binding.records.keys())
            || !names_are_valid(binding.records.values().map(|record| &record.name))
        {
            return Err("invalid stable failure, parser, or record name mapping".into());
        }
        for (id, fields) in &self.records {
            let native = &binding.records[id];
            let field_ids: BTreeSet<_> = fields.iter().map(|f| &f.id).collect();
            let traits: BTreeSet<_> = native.derives.iter().collect();
            if id.is_empty()
                || fields.is_empty()
                || fields.iter().any(|field| field.id.is_empty())
                || field_ids.len() != fields.len()
                || !field_ids.into_iter().eq(native.fields.keys())
                || !names_are_valid(native.fields.values())
                || traits.len() != native.derives.len()
                || (traits.contains(&Trait::Eq) && !traits.contains(&Trait::PartialEq))
            {
                return Err("invalid record fields or derives".into());
            }
            for field in fields {
                match &field.r#type {
                    FieldType::Unsigned { bits } | FieldType::UnsignedArray { bits, .. } => {
                        unsigned_maximum(*bits)?;
                    }
                    FieldType::Bytes { length } if *length == 0 => {
                        return Err("zero decoded field width".into());
                    }
                    FieldType::RecordVector { record } => {
                        let child = binding
                            .records
                            .get(record)
                            .ok_or("record vector names an unknown record")?;
                        if !native
                            .derives
                            .iter()
                            .all(|trait_| child.derives.contains(trait_))
                        {
                            return Err("record vector item omits a required native derive".into());
                        }
                    }
                    _ => (),
                }
            }
        }
        let mut used = BTreeSet::new();
        self.validate_constructor(&self.entry, captures, &mut used)?;
        if used.len() != self.records.len() {
            return Err("record declaration has no constructor in the result".into());
        }
        let ids: BTreeSet<_> = self.formatters.iter().map(|f| &f.id).collect();
        if ids.len() != self.formatters.len()
            || !ids.into_iter().eq(binding.formatters.keys())
            || !names_are_valid(
                binding
                    .formatters
                    .values()
                    .chain(std::iter::once(&binding.parser)),
            )
        {
            return Err("invalid formatter name mapping".into());
        }
        let entry = &self.records[&self.entry.record];
        for formatter in &self.formatters {
            if formatter.id.is_empty() || formatter.parts.is_empty()
                || formatter.argument != formatter.parts.iter().any(|part| matches!(part, Part::Argument))
                || formatter.parts.iter().any(|part| matches!(part, Part::Field {field} if !entry.iter()
                    .any(|decl| decl.id == *field && matches!(decl.r#type, FieldType::OwnedText | FieldType::StaticText))))
            {
                return Err("formatter requires text fields and a declared argument".into());
            }
        }
        Ok(())
    }

    fn validate_constructor<'a>(
        &self,
        constructor: &'a Constructor,
        captures: &Captures<'_>,
        used: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        let fields = self
            .records
            .get(&constructor.record)
            .ok_or("result constructs an unknown record")?;
        used.insert(&constructor.record);
        if constructor.fields.len() != fields.len() {
            return Err("constructor must bind each declared field".into());
        }
        let capture = |id: &str| {
            captures
                .get(id)
                .copied()
                .ok_or_else(|| format!("unknown output capture: {id}"))
        };
        for field in fields {
            let value = constructor
                .fields
                .get(&field.id)
                .ok_or("constructor does not bind a declared field")?;
            match (&field.r#type, value) {
                (ty, Value::Capture { id }) if ty.scalar(capture(id)?) => (),
                (FieldType::UnsignedArray { bits, length }, Value::Array { captures: ids })
                    if ids.len() == usize::from(*length) =>
                {
                    for id in ids {
                        if capture(id)? != Kind::Decimal(*bits) {
                            return Err("array capture width differs from field width".into());
                        }
                    }
                }
                (FieldType::RecordVector { record }, Value::Vector { items }) => {
                    for child in items {
                        if child.record != *record {
                            return Err("vector item differs from declared record type".into());
                        }
                        self.validate_constructor(child, captures, used)?;
                    }
                }
                _ => return Err(format!("incompatible typed output field: {}", field.id)),
            }
        }
        Ok(())
    }

    pub(super) fn declarations(&self, binding: &RecordBinding, source: &mut String) {
        let vis = binding.visibility.native();
        for (id, fields) in &self.records {
            let native = &binding.records[id];
            if !native.derives.is_empty() {
                let derives = native
                    .derives
                    .iter()
                    .map(Trait::native)
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(source, "#[derive({derives})]").unwrap();
            }
            writeln!(source, "{vis} struct {} {{", native.name).unwrap();
            for field in fields {
                writeln!(
                    source,
                    "    pub {}: {},",
                    native.fields[&field.id],
                    field.r#type.rust_type(&binding.records)
                )
                .unwrap();
            }
            source.push_str("}\n");
        }
        let entry = &binding.records[&self.entry.record];
        let error = binding.error.r#type.join("::");
        let variant = &binding.error.variant;
        writeln!(source, "impl {} {{", entry.name).unwrap();
        writeln!(source, "    {vis} fn {}(input: &[::core::primitive::u8]) -> ::core::result::Result<Self, {error}> {{",
            binding.parser).unwrap();
        writeln!(
            source,
            "        __parse(input).ok_or({error}::{variant}({:?}))\n    }}",
            self.failure.code
        )
        .unwrap();
        for formatter in &self.formatters {
            let argument = if formatter.argument {
                ", argument: &::core::primitive::str"
            } else {
                ""
            };
            writeln!(
                source,
                "    {vis} fn {}(&self{argument}) -> ::std::string::String {{",
                binding.formatters[&formatter.id]
            )
            .unwrap();
            source.push_str("        let mut output = ::std::string::String::new();\n");
            for part in &formatter.parts {
                let expression = match part {
                    Part::Literal { text } => format!("{text:?}"),
                    Part::Field { field } => format!("&self.{}", entry.fields[field]),
                    Part::Argument => "argument".into(),
                };
                writeln!(source, "        output.push_str({expression});").unwrap();
            }
            source.push_str("        output\n    }\n");
        }
        writeln!(
            source,
            "}}\nfn __parse(input: &[::core::primitive::u8]) -> __Option<{}> {{",
            entry.name
        )
        .unwrap();
    }

    fn constructor(
        &self,
        input: &Constructor,
        binding: &RecordBinding,
        captures: &BTreeMap<&str, usize>,
    ) -> String {
        let native = &binding.records[&input.record];
        let local = |id: &str| format!("__capture_{}", captures[id]);
        let values = self.records[&input.record]
            .iter()
            .map(|field| {
                let value = match &input.fields[&field.id] {
                    Value::Capture { id } if matches!(field.r#type, FieldType::OwnedText) => {
                        format!("::std::string::String::from({})", local(id))
                    }
                    Value::Capture { id } => local(id),
                    Value::Array { captures: ids } => {
                        let elements = ids
                            .iter()
                            .map(|id| local(id))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("[{elements}]")
                    }
                    Value::Vector { items } => {
                        let elements = items
                            .iter()
                            .map(|child| self.constructor(child, binding, captures))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("::std::vec![{elements}]")
                    }
                };
                format!("{}: {value}", native.fields[&field.id])
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} {{ {values} }}", native.name)
    }

    pub(super) fn root(&self, binding: &RecordBinding, captures: &BTreeMap<&str, usize>) -> String {
        self.constructor(&self.entry, binding, captures)
    }
}
