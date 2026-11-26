//! Finite error cases with a closed choice of payload and display rules.
use crate::native_package::digest;
use crate::shared_declarations::unique_map;
use oxigraph::model::NamedNode;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

mod recover;
mod typed;
pub use recover::recover;

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(deserialize_with = "unique_map")]
    variants: BTreeMap<String, Variant>,
    #[serde(deserialize_with = "unique_map")]
    categories: BTreeMap<String, Table>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    choices: BTreeMap<String, Vec<ChoiceCase>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    equality: bool,
}

#[derive(Deserialize, Serialize)]
struct Table(#[serde(deserialize_with = "unique_map")] BTreeMap<String, String>);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChoiceCase {
    id: String,
    text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductField {
    id: String,
    width: Width,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Width {
    U8,
    U16,
    U32,
    U64,
}

impl Width {
    fn rust(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Variant {
    payload: Payload,
    message: Message,
    source: bool,
    from: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", deny_unknown_fields)]
enum Payload {
    Unit,
    OwnedText,
    StaticText,
    IoError,
    Category { table: String },
    Choice { choice: String },
    Product { fields: Vec<ProductField> },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", deny_unknown_fields)]
enum Message {
    Literal { text: String },
    Insertion { prefix: String, suffix: String },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Overlay {
    format: String,
    #[serde(deserialize_with = "unique_map")]
    names: BTreeMap<String, String>,
    variant_order: Vec<String>,
    #[serde(deserialize_with = "unique_map")]
    category_types: BTreeMap<String, Vec<String>>,
}

fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}

fn identifier(name: &str) -> bool {
    syn::parse_str::<syn::Ident>(name).is_ok_and(|value| value == name)
        && !name.starts_with("r#")
        && name != "_"
}

fn validate(graph: &Graph, overlay: &Overlay) -> Result<()> {
    if graph.format != "oddlang-finite-error-domain-v1"
        || overlay.format != "oddlang-finite-error-rust-v1"
        || graph.variants.is_empty()
        || (graph.result.is_none()
            && graph
                .variants
                .values()
                .any(|variant| !matches!(variant.payload, Payload::Unit)))
    {
        return Err("unsupported or empty finite error domain".into());
    }
    let mut ids = BTreeSet::new();
    let mut add = |id: &String| -> Result<()> {
        NamedNode::new(id).map_err(|error| error.to_string())?;
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate error-domain identity {id}"));
        }
        Ok(())
    };
    add(&graph.error)?;
    if let Some(result) = &graph.result {
        add(result)?;
    }
    for id in graph.variants.keys() {
        add(id)?;
    }
    for (id, Table(values)) in &graph.categories {
        add(id)?;
        if values.is_empty() {
            return Err("empty category table".into());
        }
        for key in values.keys() {
            add(key)?;
        }
    }
    for (id, cases) in &graph.choices {
        add(id)?;
        if cases.is_empty() {
            return Err("empty error payload choice".into());
        }
        for case in cases {
            add(&case.id)?;
        }
    }
    for variant in graph.variants.values() {
        if let Payload::Product { fields } = &variant.payload {
            if fields.is_empty() {
                return Err("empty named error payload".into());
            }
            for field in fields {
                add(&field.id)?;
            }
        }
    }
    NamedNode::new(&graph.id).map_err(|error| error.to_string())?;
    if ids.contains(&graph.id)
        || !ids.iter().eq(overlay.names.keys())
        || !overlay.names.values().all(|name| identifier(name))
        || !graph.categories.keys().eq(overlay.category_types.keys())
        || overlay.variant_order.len() != graph.variants.len()
        || overlay.variant_order.iter().collect::<BTreeSet<_>>() != graph.variants.keys().collect()
    {
        return Err("incomplete or inconsistent finite error binding".into());
    }
    fn distinct<'a>(names: impl IntoIterator<Item = &'a String>) -> bool {
        let mut seen = BTreeSet::new();
        names.into_iter().all(|name| seen.insert(name))
    }
    if graph
        .result
        .as_ref()
        .is_some_and(|result| overlay.names[result] == overlay.names[&graph.error])
        || !distinct(graph.variants.keys().map(|id| &overlay.names[id]))
        || !distinct(graph.categories.keys().map(|id| &overlay.names[id]))
    {
        return Err("duplicate native name within error declaration scope".into());
    }
    if !distinct(
        std::iter::once(&graph.error)
            .chain(graph.result.iter())
            .chain(graph.choices.keys())
            .map(|id| &overlay.names[id]),
    ) || graph
        .choices
        .values()
        .any(|cases| !distinct(cases.iter().map(|case| &overlay.names[&case.id])))
        || graph
            .variants
            .values()
            .any(|variant| match &variant.payload {
                Payload::Product { fields } => {
                    !distinct(fields.iter().map(|field| &overlay.names[&field.id]))
                }
                _ => false,
            })
    {
        return Err("duplicate native error choice or product name".into());
    }
    for (id, Table(table)) in &graph.categories {
        let path = &overlay.category_types[id];
        if path.len() < 2
            || !path
                .iter()
                .enumerate()
                .all(|(index, item)| identifier(item) || (index == 0 && item == "crate"))
            || !distinct(table.keys().map(|key| &overlay.names[key]))
        {
            return Err(format!("invalid native category type or member name: {id}"));
        }
    }
    let mut tables = BTreeSet::new();
    let mut choices = BTreeSet::new();
    let mut conversion = false;
    for variant in graph.variants.values() {
        let inserts = matches!(variant.message, Message::Insertion { .. });
        match &variant.payload {
            Payload::Category { table } if graph.categories.contains_key(table) => {
                tables.insert(table);
            }
            Payload::Category { .. } => return Err("unknown category table".into()),
            Payload::Choice { choice } if graph.choices.contains_key(choice) => {
                choices.insert(choice);
            }
            Payload::Choice { .. } => return Err("unknown error payload choice".into()),
            Payload::Unit | Payload::IoError | Payload::Product { .. } if inserts => {
                return Err("display cannot insert this payload".into());
            }
            _ => {}
        }
        if graph.equality && matches!(variant.payload, Payload::IoError | Payload::Category { .. })
        {
            return Err("opaque error payload does not declare equality".into());
        }
        if (variant.source || variant.from) && !matches!(variant.payload, Payload::IoError) {
            return Err("error source or conversion requires an I/O error payload".into());
        }
        if variant.from && (!variant.source || std::mem::replace(&mut conversion, true)) {
            return Err("conflicting I/O error conversion".into());
        }
    }
    if !choices.into_iter().eq(graph.choices.keys()) {
        return Err("unused error payload choice".into());
    }
    if !tables.into_iter().eq(graph.categories.keys()) {
        return Err("unused category table".into());
    }
    Ok(())
}

fn native_path(path: &[String]) -> String {
    if path[0] == "crate" {
        path.join("::")
    } else {
        format!("::{}", path.join("::"))
    }
}

fn render(graph: &Graph, overlay: &Overlay) -> String {
    let names = &overlay.names;
    let name = &names[&graph.error];
    let mut out = String::new();
    for (id, cases) in &graph.choices {
        crate::checked_values::render_plain_choice(
            &mut out,
            &names[id],
            cases.iter().map(|case| names[&case.id].as_str()),
            crate::checked_values::Equality::Total,
            true,
        );
    }
    let equality = if graph.equality {
        ", PartialEq, Eq"
    } else {
        ""
    };
    writeln!(out, "#[derive(Debug{equality})]\npub enum {name} {{").unwrap();
    let variants = overlay
        .variant_order
        .iter()
        .map(|id| (&names[id], &graph.variants[id]));
    for (name, variant) in variants.clone() {
        let payload = match &variant.payload {
            Payload::Unit => String::new(),
            Payload::OwnedText => "(::std::string::String)".into(),
            Payload::StaticText => "(&'static ::core::primitive::str)".into(),
            Payload::IoError => "(::std::io::Error)".into(),
            Payload::Category { table } => {
                format!("({})", native_path(&overlay.category_types[table]))
            }
            Payload::Choice { choice } => format!("(self::{})", names[choice]),
            Payload::Product { fields } => {
                let fields = fields
                    .iter()
                    .map(|field| {
                        format!(
                            "{}: ::core::primitive::{}",
                            names[&field.id],
                            field.width.rust()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" {{ {fields} }}")
            }
        };
        writeln!(out, "    {name}{payload},").unwrap();
    }
    if let Some(result) = &graph.result {
        let alias = &names[result];
        writeln!(
            out,
            "}}\npub type {alias}<T> = ::core::result::Result<T, self::{name}>;\n"
        )
        .unwrap();
    } else {
        out.push_str("}\n");
    }
    writeln!(out, "impl ::core::fmt::Display for {name} {{\n    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{\n        match self {{").unwrap();
    for (name, variant) in variants.clone() {
        let arg = match (&variant.payload, &variant.message) {
            (Payload::Unit, _) => "",
            (Payload::Product { .. }, _) => " { .. }",
            (_, Message::Literal { .. }) => "(_)",
            (_, Message::Insertion { .. }) => "(value)",
        };
        let body = match &variant.message {
            Message::Literal { text } => format!("formatter.write_str({text:?})"),
            Message::Insertion { prefix, suffix } => {
                let value = match &variant.payload {
                    Payload::Category { table } => format!("self::{}(value)", names[table]),
                    Payload::Choice { choice } => {
                        let mut expression = "match value {".to_owned();
                        for case in &graph.choices[choice] {
                            write!(
                                expression,
                                " self::{}::{} => {:?},",
                                names[choice], names[&case.id], case.text
                            )
                            .unwrap();
                        }
                        expression.push_str(" }");
                        expression
                    }
                    _ => "value".to_owned(),
                };
                format!(
                    "{{ formatter.write_str({prefix:?})?; formatter.write_str({value})?; formatter.write_str({suffix:?}) }}"
                )
            }
        };
        writeln!(out, "            Self::{name}{arg} => {body},").unwrap();
    }
    out.push_str("        }\n    }\n}\n\n");
    writeln!(out, "impl ::core::error::Error for {name} {{\n    fn source(&self) -> ::core::option::Option<&(dyn ::core::error::Error + 'static)> {{\n        match self {{").unwrap();
    for (name, variant) in variants.clone() {
        let (arg, body) = if variant.source {
            ("(source)", "::core::option::Option::Some(source)")
        } else {
            let arg = match variant.payload {
                Payload::Unit => "",
                Payload::Product { .. } => " { .. }",
                _ => "(_)",
            };
            (arg, "::core::option::Option::None")
        };
        writeln!(out, "            Self::{name}{arg} => {body},").unwrap();
    }
    out.push_str("        }\n    }\n}\n");
    for (variant_name, variant) in variants {
        if variant.from {
            writeln!(out, "\nimpl ::core::convert::From<::std::io::Error> for {name} {{\n    fn from(error: ::std::io::Error) -> Self {{\n        Self::{variant_name}(error)\n    }}\n}}").unwrap();
        }
    }
    for (id, Table(table)) in &graph.categories {
        let ty = native_path(&overlay.category_types[id]);
        writeln!(out, "\n#[allow(dead_code)]\npub(crate) fn {}(code: &{ty}) -> &'static ::core::primitive::str {{\n    match code {{", names[id]).unwrap();
        for (key, text) in table {
            writeln!(out, "        {ty}::{} => {text:?},", names[key]).unwrap();
        }
        out.push_str("    }\n}\n");
    }
    out
}

fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn publish(output: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    if output.symlink_metadata().is_ok() {
        return Err("finite error output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = io(tempfile::tempdir_in(parent))?;
    for (name, bytes) in files {
        io(fs::write(stage.path().join(name), bytes))?;
    }
    crate::native_package::publish(stage.path(), output)
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    build_bytes(&io(fs::read(graph))?, &io(fs::read(overlay))?, output)
}

pub(crate) fn error_variants(
    graph: &[u8],
    overlay: &[u8],
) -> Result<Vec<crate::native_package::ErrorVariantExport>> {
    let graph: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let overlay: Overlay = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    validate(&graph, &overlay)?;
    Ok(graph
        .variants
        .into_iter()
        .map(|(id, variant)| crate::native_package::ErrorVariantExport {
            unit: matches!(variant.payload, Payload::Unit),
            static_text: matches!(variant.payload, Payload::StaticText),
            error_type: overlay.names[&graph.error].clone(),
            variant: overlay.names[&id].clone(),
            id,
        })
        .collect())
}

pub(crate) fn build_bytes(graph: &[u8], overlay: &[u8], output: &Path) -> Result<()> {
    let parsed_graph: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let parsed_overlay: Overlay = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    validate(&parsed_graph, &parsed_overlay)?;
    let generated = render(&parsed_graph, &parsed_overlay);
    let cases = json(&parsed_graph)?;
    let receipt = json(&serde_json::json!({
        "format": "oddlang-finite-error-lock-v1",
        "graphSha256": digest(graph),
        "overlaySha256": digest(overlay),
        "obligationsSha256": digest(&cases),
        "rustSha256": digest(generated.as_bytes()),
    }))?;
    publish(
        output,
        &[
            ("errors.rs", generated.as_bytes()),
            ("obligations.json", &cases),
            ("projection.lock", &receipt),
        ],
    )
}
