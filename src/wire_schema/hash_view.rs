use super::{
    Binding, Definition, Field, Predicate, Result, Schema, Shape, distinct, iri, symbol,
    type_symbol,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct View {
    pub(super) id: String,
    pub(super) owner: String,
    algorithm: Algorithm,
    domain: Vec<u8>,
    #[serde(default)]
    validate: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parameters: Vec<Parameter>,
    fields: Vec<Slot>,
    output: Output,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    id: String,
    external: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Algorithm {
    Sha256,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
enum Slot {
    Field {
        field: String,
        encoding: Encoding,
    },
    Parameter {
        parameter: String,
        encoding: Encoding,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Encoding {
    UnsignedBe,
    BoolU8,
    FixedBytes,
    U32BeLengthPrefixedBytes,
    ExternalTagU8,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Output {
    Bytes {},
    LowerHex { prefix: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ViewBinding {
    method: String,
    library: Library,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    parameters: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Library {
    Sha2,
}

pub(super) fn validate(
    schema: &Schema,
    binding: &Binding,
    declared: &BTreeMap<&str, &Definition>,
    ids: &BTreeSet<&str>,
) -> Result<()> {
    if !schema
        .hash_views
        .iter()
        .map(|v| &v.id)
        .eq(binding.hash_views.keys())
    {
        return Err("hash views require exactly one Rust binding per distinct identity".into());
    }
    let mut tags = BTreeSet::new();
    for (id, table) in &schema.hash_tags {
        if !matches!(declared.get(id.as_str()), Some(Definition::External { .. }))
            || table.is_empty()
            || !distinct(table.values())
            || !distinct(
                table
                    .keys()
                    .filter_map(|case| binding.hash_variants.get(case)),
            )
        {
            return Err(format!("invalid hash tag table: {id}"));
        }
        for case in table.keys() {
            iri(case)?;
            if !tags.insert(case)
                || !binding
                    .hash_variants
                    .get(case)
                    .is_some_and(|name| type_symbol(name))
            {
                return Err(format!("invalid hash variant binding: {case}"));
            }
        }
    }
    if !tags.into_iter().eq(binding.hash_variants.keys()) {
        return Err("hash variant bindings must cover precisely the tag tables".into());
    }
    let mut methods = BTreeSet::new();
    let mut used_tables = BTreeSet::new();
    for view in &schema.hash_views {
        iri(&view.id)?;
        if ids.contains(view.id.as_str()) || schema.guards.iter().any(|g| g.id == view.id) {
            return Err(format!("duplicate hash view identity: {}", view.id));
        }
        let Some(Definition::Record { fields, .. }) = declared.get(view.owner.as_str()) else {
            return Err(format!("hash view requires a record owner: {}", view.id));
        };
        let native = &binding.hash_views[&view.id];
        let guard_method = binding
            .native_validation
            .get(&view.owner)
            .map_or("validate_wire", |guard| guard.method.as_str());
        let has_guards = schema.guards.iter().any(|guard| guard.owner == view.owner);
        if !symbol(&native.method)
            || !methods.insert((&view.owner, &native.method))
            || (has_guards && native.method == guard_method)
        {
            return Err(format!(
                "invalid or duplicate hash method binding: {}",
                view.id
            ));
        }
        if view.fields.is_empty() || (view.validate && !has_guards) {
            return Err(format!(
                "hash view requires slots and any selected validation: {}",
                view.id
            ));
        }
        let parameters: BTreeMap<_, _> = view
            .parameters
            .iter()
            .map(|p| (&p.id, &p.external))
            .collect();
        if parameters.len() != view.parameters.len()
            || !parameters.keys().copied().eq(native.parameters.keys())
            || !distinct(native.parameters.values())
            || native
                .parameters
                .values()
                .any(|param| !symbol(param) || !param.as_bytes()[0].is_ascii_lowercase())
        {
            return Err(format!("invalid hash parameter binding: {}", view.id));
        }
        let mut referenced_parameters = BTreeSet::new();
        for slot in &view.fields {
            match slot {
                Slot::Field { field, encoding } => {
                    let field = fields
                        .iter()
                        .find(|candidate| &candidate.id == field)
                        .ok_or_else(|| format!("unknown hash slot field: {field}"))?;
                    if let Some(table) = validate_field(schema, view, field, *encoding)? {
                        used_tables.insert(table);
                    }
                }
                Slot::Parameter {
                    parameter,
                    encoding,
                } => {
                    iri(parameter)?;
                    let table = parameters
                        .get(parameter)
                        .filter(|id| schema.hash_tags.contains_key(**id));
                    if let (Encoding::ExternalTagU8, Some(table)) = (encoding, table) {
                        used_tables.insert(table.as_str());
                    } else {
                        return Err(format!(
                            "hash slot type does not support parameter encoding: {parameter}"
                        ));
                    }
                    referenced_parameters.insert(parameter);
                }
            }
        }
        if !referenced_parameters
            .into_iter()
            .eq(parameters.keys().copied())
        {
            return Err(format!("unused hash parameter: {}", view.id));
        }
    }
    if !used_tables
        .into_iter()
        .eq(schema.hash_tags.keys().map(String::as_str))
    {
        return Err("unused hash tag table".into());
    }
    Ok(())
}

fn validate_field<'a>(
    schema: &Schema,
    view: &View,
    field: &'a Field,
    encoding: Encoding,
) -> Result<Option<&'a str>> {
    match (&field.shape, encoding) {
        (Shape::Unsigned { .. }, Encoding::UnsignedBe)
        | (Shape::Bool {}, Encoding::BoolU8)
        | (Shape::LowerHex { .. }, Encoding::FixedBytes) => Ok(None),
        (Shape::BoundedBase64 { .. }, Encoding::U32BeLengthPrefixedBytes) => {
            // Construction can bypass JSON byte bounds. The invoked, unconditional
            // count guard itself has a u32 maximum and runs before serialization.
            if !view.validate || !schema.guards.iter().any(|g| {
                g.owner == view.owner
                    && g.when.is_none()
                    && matches!(&g.require, Predicate::Count { field: id, .. } if id == &field.id)
            }) {
                return Err(format!(
                    "length-prefixed hash slot needs an unconditional count guard: {}",
                    field.id
                ));
            }
            Ok(None)
        }
        (Shape::Ref { id }, Encoding::ExternalTagU8) if schema.hash_tags.contains_key(id) => {
            Ok(Some(id))
        }
        _ => Err(format!("invalid hash slot type: {}", field.id)),
    }
}

pub(super) fn render(
    text: &mut String,
    schema: &Schema,
    binding: &Binding,
    id: &str,
    fields: &[Field],
) {
    for view in schema.hash_views.iter().filter(|view| view.owner == id) {
        let native = &binding.hash_views[&view.id];
        let name = &binding.names[id];
        let declared_result = match view.output {
            Output::Bytes {} => "[::core::primitive::u8; 32]",
            Output::LowerHex { .. } => "::std::string::String",
        };
        let (check, returns) = if view.validate {
            let (method, error) = binding
                .native_validation
                .get(id)
                .map_or(("validate_wire", "&'static str"), |n| {
                    (n.method.as_str(), n.type_path.as_str())
                });
            (
                Some(method),
                format!("::core::result::Result<{declared_result}, {error}>"),
            )
        } else {
            (None, declared_result.to_owned())
        };
        write!(text, "impl {name} {{\n    pub fn {}(&self", native.method).unwrap();
        for parameter in &view.parameters {
            write!(
                text,
                ", {}: {}",
                native.parameters[&parameter.id], binding.externals[&parameter.external]
            )
            .unwrap();
        }
        writeln!(text, ") -> {returns} {{").unwrap();
        if let Some(method) = check {
            writeln!(text, "        self.{method}()?;").unwrap();
        }
        match (&view.algorithm, &native.library) {
            (Algorithm::Sha256, Library::Sha2) => text.push_str(
                "        use ::sha2::Digest as _;\n        let mut __oddlang_hash = ::sha2::Sha256::new();\n"
            ),
        }
        writeln!(text, "        __oddlang_hash.update({:?});", view.domain).unwrap();
        for slot in &view.fields {
            let (encoding, value, tag_table) = match slot {
                Slot::Field { field, encoding } => {
                    let field = fields
                        .iter()
                        .find(|f| &f.id == field)
                        .expect("validated field");
                    let table = match &field.shape {
                        Shape::Ref { id } => Some(id),
                        _ => None,
                    };
                    (
                        *encoding,
                        format!("self.{}", binding.names[&field.id]),
                        table,
                    )
                }
                Slot::Parameter {
                    parameter,
                    encoding,
                } => (
                    *encoding,
                    native.parameters[parameter].clone(),
                    Some(
                        &view
                            .parameters
                            .iter()
                            .find(|p| p.id == *parameter)
                            .expect("validated parameter")
                            .external,
                    ),
                ),
            };
            render_slot(text, schema, binding, encoding, &value, tag_table);
        }
        let result = match &view.output {
            Output::Bytes {} => "__oddlang_hash.finalize().into()",
            Output::LowerHex { prefix } => {
                text.push_str("        use ::core::fmt::Write as _;\n");
                writeln!(
                    text,
                    "        let mut __oddlang_text = ::std::string::String::from({prefix:?});"
                )
                .unwrap();
                text.push_str("        for __oddlang_byte in __oddlang_hash.finalize() {\n            write!(__oddlang_text, \"{__oddlang_byte:02x}\").expect(\"format into String\");\n        }\n");
                "__oddlang_text"
            }
        };
        if check.is_some() {
            writeln!(text, "        ::core::result::Result::Ok({result})").unwrap();
        } else {
            writeln!(text, "        {result}").unwrap();
        }
        text.push_str("    }\n}\n\n");
    }
}

fn render_slot(
    text: &mut String,
    schema: &Schema,
    binding: &Binding,
    encoding: Encoding,
    value: &str,
    tag_table: Option<&String>,
) {
    match encoding {
        Encoding::UnsignedBe => writeln!(
            text,
            "        __oddlang_hash.update({value}.to_be_bytes());"
        )
        .unwrap(),
        Encoding::BoolU8 => writeln!(
            text,
            "        __oddlang_hash.update([::core::primitive::u8::from({value})]);"
        )
        .unwrap(),
        Encoding::FixedBytes => writeln!(text, "        __oddlang_hash.update({value});").unwrap(),
        Encoding::U32BeLengthPrefixedBytes => {
            writeln!(text, "        __oddlang_hash.update(::core::primitive::u32::try_from({value}.len()).expect(\"validated byte count\").to_be_bytes());").unwrap();
            writeln!(text, "        __oddlang_hash.update(&{value});").unwrap();
        }
        Encoding::ExternalTagU8 => {
            let table = tag_table.expect("validated external tag");
            let ty = &binding.externals[table];
            writeln!(text, "        __oddlang_hash.update([match &{value} {{").unwrap();
            for (case, tag) in &schema.hash_tags[table] {
                writeln!(
                    text,
                    "            {ty}::{} => {tag}u8,",
                    binding.hash_variants[case]
                )
                .unwrap();
            }
            text.push_str("        }]);\n");
        }
    }
}
