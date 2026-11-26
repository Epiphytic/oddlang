//! Recover a named Serde record with stable links checked against compiler evidence.
use super::*;
use serde_json::{Value as Json, json};
use syn::{
    Attribute, GenericArgument, Item, ItemStruct, PathArguments, Token, Type, Visibility,
    ext::IdentExt, punctuated::Punctuated, spanned::Spanned,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    crate_name: String,
    module_name: String,
    module_id: String,
    source_filename: String,
    source_sha256: String,
    crate_root_sha256: String,
    rustdoc_sha256: String,
    record: RecordSelection,
    choices: Vec<ChoiceSelection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordSelection {
    name: String,
    id: String,
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChoiceSelection {
    name: String,
    id: String,
}

#[derive(Deserialize)]
struct Document {
    format_version: u64,
    includes_private: bool,
    root: u64,
    #[serde(deserialize_with = "unique_map")]
    index: BTreeMap<String, DocumentItem>,
    #[serde(deserialize_with = "unique_map")]
    paths: BTreeMap<String, DocumentPath>,
}

#[derive(Deserialize)]
struct DocumentPath {
    crate_id: u64,
    path: Vec<String>,
    kind: String,
}

#[derive(Deserialize)]
struct DocumentItem {
    id: u64,
    crate_id: u64,
    name: Option<String>,
    span: Option<SourceSpan>,
    visibility: Json,
    attrs: Vec<Json>,
    inner: Json,
}

#[derive(Deserialize)]
struct SourceSpan {
    filename: String,
    begin: [usize; 2],
    end: [usize; 2],
}

impl Document {
    fn item(&self, id: u64) -> Result<&DocumentItem> {
        self.index
            .get(&id.to_string())
            .filter(|i| i.id == id)
            .ok_or_else(|| format!("missing or mismatched rustdoc item: {id}"))
    }

    fn children(&self, module: &DocumentItem) -> Result<Vec<&DocumentItem>> {
        ids(&module.inner["module"]["items"])?
            .into_iter()
            .map(|id| self.item(id))
            .collect()
    }

    fn path(&self, id: u64) -> Result<&DocumentPath> {
        self.paths
            .get(&id.to_string())
            .ok_or_else(|| format!("missing rustdoc path: {id}"))
    }
}

fn ids(array: &Json) -> Result<Vec<u64>> {
    array
        .as_array()
        .ok_or("missing rustdoc item list")?
        .iter()
        .map(|id| id.as_u64().ok_or_else(|| "invalid rustdoc item ID".into()))
        .collect()
}

fn coordinates(location: proc_macro2::LineColumn) -> [usize; 2] {
    [location.line, location.column + 1]
}

fn has_span(
    item: &DocumentItem,
    source: &str,
    start: proc_macro2::Span,
    end: proc_macro2::Span,
) -> bool {
    item.span.as_ref().is_some_and(|actual| {
        actual.filename == source
            && actual.begin == coordinates(start.start())
            && actual.end == coordinates(end.end())
    })
}

fn match_span(
    item: &DocumentItem,
    source: &str,
    start: proc_macro2::Span,
    end: proc_macro2::Span,
) -> Result<()> {
    if has_span(item, source, start, end) {
        Ok(())
    } else {
        Err("source and rustdoc spans disagree".into())
    }
}

fn named_local<'a>(items: &[&'a DocumentItem], name: &str) -> Result<&'a DocumentItem> {
    let found: Vec<_> = items
        .iter()
        .copied()
        .filter(|i| i.name.as_deref() == Some(name))
        .collect();
    match found.as_slice() {
        [item] if item.crate_id == 0 => Ok(item),
        _ => Err(format!(
            "expected one local rustdoc declaration named {name}"
        )),
    }
}

fn root_exports(
    document: &Document,
    root_items: &[&DocumentItem],
    owner: u64,
    item: &DocumentItem,
    root_source: &syn::File,
) -> Result<()> {
    let name = item
        .name
        .as_deref()
        .ok_or("missing exported declaration name")?;
    let mut providers = Vec::new();
    let root_filename = document
        .item(document.root)?
        .span
        .as_ref()
        .ok_or("missing crate-root source span")?
        .filename
        .as_str();
    if root_source.items.iter().any(|entry| {
        recover::item_type_binding(entry)
            .is_some_and(|(ident, _)| ident.unraw().to_string() == name)
    }) {
        return Err(format!(
            "crate-root declaration can shadow the crate-root export of {name}"
        ));
    }
    let source_imports = recover::imports(root_source);
    if source_imports.iter().any(|import| {
        !matches!(import.declaration.vis, Visibility::Public(_))
            && (import.name == name || import.name == "*")
    }) {
        return Err(format!(
            "private crate-root import can shadow the crate-root export of {name}"
        ));
    }
    let checked_use = |entry: &DocumentItem, spelling: &str, path: &[&str]| {
        source_imports.iter().any(|import| {
            import.name == spelling
                && import.path == path
                && matches!(import.declaration.vis, Visibility::Public(_))
                && import.declaration.attrs.iter().all(doc_attribute)
                && has_span(
                    entry,
                    root_filename,
                    import.declaration.vis.span(),
                    import.declaration.semi_token.span(),
                )
        })
    };
    for entry in root_items {
        let public = entry.visibility == "public";
        if let Some(use_item) = entry.inner.get("use") {
            if use_item["is_glob"] == true {
                if !public {
                    return Err(format!(
                        "private crate-root glob can shadow the crate-root export of {name}"
                    ));
                }
                let target =
                    document.item(use_item["id"].as_u64().ok_or("missing root glob target")?)?;
                if target.crate_id != 0 {
                    return Err("foreign root glob has unverified exports".into());
                }
                for child in document
                    .children(target)?
                    .into_iter()
                    .filter(|child| child.visibility == "public")
                {
                    if child.inner.get("use").is_some() {
                        return Err("nested root glob has unverified exports".into());
                    }
                    if child.name.as_deref() == Some(name) {
                        if target.id == owner
                            && !checked_use(
                                entry,
                                "*",
                                &[target.name.as_deref().ok_or("missing source module name")?],
                            )
                        {
                            return Err("root glob does not match the pinned root source".into());
                        }
                        providers.push((target.id, child.id));
                    }
                }
            } else if use_item["name"] == name {
                if !public {
                    return Err(format!(
                        "private crate-root import shadows the crate-root export of {name}"
                    ));
                }
                let owner_name = document
                    .item(owner)?
                    .name
                    .as_deref()
                    .ok_or("missing source module name")?;
                if !checked_use(entry, name, &[owner_name, name])
                    && !checked_use(entry, name, &["crate", owner_name, name])
                {
                    return Err("root reexport does not match the pinned root source".into());
                }
                providers.push((
                    owner,
                    use_item["id"]
                        .as_u64()
                        .ok_or("missing root reexport target")?,
                ));
            }
        } else if entry.name.as_deref() == Some(name) {
            providers.push((document.root, entry.id));
        }
    }
    if providers != [(owner, item.id)] {
        return Err(format!("expected one crate-root export of {name}"));
    }
    Ok(())
}

fn doc_attribute(attr: &Attribute) -> bool {
    attr.path().is_ident("doc")
}

const STOCK_DERIVES: &[&str] = &[
    "Clone",
    "Debug",
    "PartialEq",
    "Eq",
    "Serialize",
    "Deserialize",
];
const RESERVED_TYPES: &[&str] = &[
    "serde", "core", "std", "Option", "bool", "u32", "u64", "usize", "derive",
];

fn reject_shadowing_declarations(items: &[Item]) -> Result<()> {
    for item in items {
        let name = match item {
            Item::Macro(item) => item.ident.as_ref(),
            _ => recover::item_type_binding(item).map(|(name, _)| name),
        };
        if name.is_some_and(|name| {
            let name = name.unraw().to_string();
            RESERVED_TYPES.contains(&name.as_str()) || STOCK_DERIVES.contains(&name.as_str())
        }) {
            return Err("shadowed type or derive in record source".into());
        }
    }
    Ok(())
}

fn stock_traits(
    source: &syn::File,
    record: &ItemStruct,
    witness: &DocumentItem,
    document: &Document,
    filename: &str,
) -> Result<(Vec<Trait>, UnknownFields)> {
    let imports = recover::imports(source);
    reject_shadowing_declarations(&source.items)?;
    for import in &imports {
        if import.name == "*" || RESERVED_TYPES.contains(&import.name.as_str()) {
            return Err("unverified glob or shadowed type or derive in record source".into());
        }
        if STOCK_DERIVES.contains(&import.name.as_str())
            && (!matches!(import.name.as_str(), "Serialize" | "Deserialize")
                || import.path != ["serde", import.name.as_str()]
                || !import.declaration.attrs.iter().all(doc_attribute)
                || imports
                    .iter()
                    .filter(|other| other.name == import.name)
                    .count()
                    != 1)
        {
            return Err("shadowed type or derive in record source".into());
        }
    }
    let impls = ids(&witness.inner["struct"]["impls"])?;
    let mut traits = Vec::new();
    let mut unknown_fields = None;
    for attr in record.attrs.iter().filter(|attr| !doc_attribute(attr)) {
        if attr.path().is_ident("serde") {
            let policy = attr
                .parse_args_with(Punctuated::<syn::Ident, Token![,]>::parse_terminated)
                .map_err(|_| "unsupported Serde record attribute")?;
            if policy.len() != 1
                || policy[0] != "deny_unknown_fields"
                || unknown_fields.replace(UnknownFields::Reject).is_some()
            {
                return Err("unsupported or duplicate Serde record attribute".into());
            }
            continue;
        }
        if !attr.path().is_ident("derive") {
            return Err("unsupported record attribute".into());
        }
        let names = attr
            .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
            .map_err(|err| err.to_string())?;
        for path in names {
            let spelling: Vec<_> = path
                .segments
                .iter()
                .map(|s| s.ident.unraw().to_string())
                .collect();
            let (capability, canonical): (Trait, &[&str]) = match spelling
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["Clone"] => (Trait::Clone, &["core", "clone", "Clone"]),
                ["Debug"] => (Trait::Debug, &["core", "fmt", "Debug"]),
                ["PartialEq"] => (Trait::PartialEq, &["core", "cmp", "PartialEq"]),
                ["Eq"] => (Trait::Eq, &["core", "cmp", "Eq"]),
                [serde @ ("Serialize" | "Deserialize")]
                    if imports.iter().any(|item| item.name == *serde) =>
                {
                    if *serde == "Serialize" {
                        (Trait::Serialize, &["serde_core", "ser", "Serialize"])
                    } else {
                        (Trait::Deserialize, &["serde_core", "de", "Deserialize"])
                    }
                }
                ["serde", "Serialize"] => (Trait::Serialize, &["serde_core", "ser", "Serialize"]),
                ["serde", "Deserialize"] => {
                    (Trait::Deserialize, &["serde_core", "de", "Deserialize"])
                }
                _ => return Err("unsupported public record derive".into()),
            };
            if traits.contains(&capability) || path.segments.iter().any(|s| !s.arguments.is_empty())
            {
                return Err("duplicate or invalid public record derive".into());
            }
            let mut matches = 0;
            for id in &impls {
                let item = document.item(*id)?;
                let info = &item.inner["impl"];
                if let Some(trait_id) = info["trait"]["id"].as_u64()
                    && document
                        .path(trait_id)
                        .is_ok_and(|p| p.crate_id != 0 && p.kind == "trait" && p.path == canonical)
                    && info["for"]["resolved_path"]["id"] == witness.id
                    && info["is_synthetic"] == false
                    && info["is_negative"] == false
                    && item
                        .attrs
                        .iter()
                        .any(|attr| attr == "automatically_derived")
                    && has_span(item, filename, path.span(), path.span())
                {
                    matches += 1;
                }
            }
            if matches != 1 {
                return Err(format!(
                    "expected one canonical compiler-derived trait: {capability:?}"
                ));
            }
            traits.push(capability);
        }
    }
    if !traits.contains(&Trait::Serialize) || !traits.contains(&Trait::Deserialize) {
        return Err("expected both standard Serde derives on the record".into());
    }
    Ok((traits, unknown_fields.unwrap_or(UnknownFields::Ignore)))
}

fn primitive(source: &syn::Path, declared: &Json) -> Result<Value> {
    let name = declared
        .as_str()
        .ok_or("invalid compiler primitive field type")?;
    if !source.is_ident(name) {
        return Err("source and compiler primitive field types disagree".into());
    }
    match name {
        "bool" => Ok(Value::Bool),
        "u32" => Ok(Value::U32),
        "u64" => Ok(Value::U64),
        "usize" => Ok(Value::PointerUnsigned),
        _ => Err("unsupported primitive field type".into()),
    }
}

fn field_value<'a>(
    source: &Type,
    declared: &Json,
    document: &Document,
    choices: &BTreeMap<u64, &'a ChoiceSelection>,
    allow_optional: bool,
) -> Result<(Value, Option<&'a ChoiceSelection>)> {
    let Type::Path(node) = source else {
        return Err("unsupported source field type".into());
    };
    if node.qself.is_some() {
        return Err("unsupported qualified field type".into());
    }
    let path = &node.path;
    if let Some(name) = declared.get("primitive") {
        return Ok((primitive(path, name)?, None));
    }
    let resolved = &declared["resolved_path"];
    let id = resolved["id"]
        .as_u64()
        .ok_or("field declaration has no checked type identity")?;
    let target = document.path(id)?;
    if target.crate_id != 0 && target.kind == "enum" && target.path == ["core", "option", "Option"]
    {
        if !allow_optional {
            return Err("nested optional field types are unsupported".into());
        }
        let Some(last) = path.segments.last().filter(|seg| seg.ident == "Option") else {
            return Err("unsupported optional field spelling".into());
        };
        let prefix: Vec<_> = path
            .segments
            .iter()
            .take(path.segments.len() - 1)
            .map(|segment| segment.ident.unraw().to_string())
            .collect();
        if !(prefix.is_empty() || prefix == ["core", "option"] || prefix == ["std", "option"])
            || path
                .segments
                .iter()
                .take(prefix.len())
                .any(|seg| !seg.arguments.is_empty())
        {
            return Err("unsupported optional field spelling".into());
        }
        let PathArguments::AngleBracketed(args) = &last.arguments else {
            return Err("unsupported optional field syntax".into());
        };
        let source_args: Vec<_> = args.args.iter().collect();
        let [GenericArgument::Type(element)] = source_args.as_slice() else {
            return Err("expected one optional field type argument".into());
        };
        let compiler_args = &resolved["args"]["angle_bracketed"];
        let Some([arg]) = compiler_args["args"].as_array().map(Vec::as_slice) else {
            return Err("expected one compiler optional field argument".into());
        };
        if compiler_args["constraints"] != json!([]) {
            return Err("unsupported compiler optional field constraints".into());
        }
        let (value, link) = field_value(element, &arg["type"], document, choices, false)?;
        return Ok((Value::Optional(Box::new(value)), link));
    }
    if let Some(choice) = choices.get(&id)
        && target.crate_id == 0
        && target.kind == "enum"
        && path.is_ident(choice.name.as_str())
        && resolved["args"].is_null()
    {
        return Ok((Value::External(choice.id.clone()), Some(choice)));
    }
    Err("field declaration does not resolve to a selected external choice".into())
}

pub fn recover_serde(source: &Path, rustdoc: &Path, selection: &Path, out: &Path) -> Result<()> {
    let selection_bytes = io(fs::read(selection))?;
    let selected: Selection =
        serde_json::from_slice(&selection_bytes).map_err(|err| err.to_string())?;
    let document_bytes = io(fs::read(rustdoc))?;
    if selected.format != "oddlang-serde-public-record-selection-v1"
        || digest(&document_bytes) != selected.rustdoc_sha256
        || selected.source_filename.is_empty()
        || !Path::new(&selected.source_filename)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        || Path::new(&selected.source_filename)
            .file_stem()
            .and_then(|name| name.to_str())
            != Some(&selected.module_name)
        || !source.ends_with(&selected.source_filename)
    {
        return Err("invalid public record selection or compiler evidence digest".into());
    }
    let syntax = recover::select_source(source, &selected.source_sha256)?;
    let root_source = recover::select_source(
        &source.with_file_name("lib.rs"),
        &selected.crate_root_sha256,
    )?;
    if root_source
        .attrs
        .iter()
        .chain(syntax.attrs.iter())
        .any(|attr| !doc_attribute(attr))
    {
        return Err("unsupported root or module attribute for Serde record recovery".into());
    }
    if root_source
        .items
        .iter()
        .chain(&syntax.items)
        .any(|item| match item {
            Item::ExternCrate(item) => item.attrs.iter().any(|attr| !doc_attribute(attr)),
            Item::Mod(item) => item
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("macro_use") || attr.path().is_ident("cfg_attr")),
            _ => false,
        })
    {
        return Err("unverified macro imports can shadow record derives".into());
    }
    reject_shadowing_declarations(&root_source.items)?;
    let modules = root_source.items.iter().filter(|item| matches!(item,
        Item::Mod(module) if module.ident == selected.module_name && module.content.is_none()
            && module.attrs.iter().all(doc_attribute)
    )).count();
    if modules != 1 {
        return Err("expected one source module declaration in the crate root".into());
    }
    let document: Document =
        serde_json::from_slice(&document_bytes).map_err(|err| err.to_string())?;
    if !document.includes_private {
        return Err("rustdoc evidence must include private items".into());
    }
    let root = document.item(document.root)?;
    if document.format_version != 57
        || root.crate_id != 0
        || root.name.as_deref() != Some(&selected.crate_name)
        || root.visibility != "public"
        || !root.span.as_ref().is_some_and(|span| {
            Path::new(&span.filename)
                == Path::new(&selected.source_filename).with_file_name("lib.rs")
        })
        || root.inner["module"]["is_crate"] != true
        || root.inner["module"]["is_stripped"] != false
    {
        return Err("expected selected local rustdoc format 57 crate root".into());
    }
    let root_items = document.children(root)?;
    let module = named_local(&root_items, &selected.module_name)?;
    if !module
        .span
        .as_ref()
        .is_some_and(|span| span.filename == selected.source_filename)
        || module.inner["module"]["is_crate"] != false
        || module.inner["module"]["is_stripped"] != false
    {
        return Err("expected selected source module in compiler evidence".into());
    }
    let children = document.children(module)?;
    let record = named_local(&children, &selected.record.name)?;
    let records: Vec<_> = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(item) if item.ident == selected.record.name => Some(item),
            _ => None,
        })
        .collect();
    let [declaration] = records.as_slice() else {
        return Err("expected one source record declaration".into());
    };
    root_exports(&document, &root_items, module.id, record, &root_source)?;
    let syn::Fields::Named(fields) = &declaration.fields else {
        return Err("expected named source record fields".into());
    };
    match_span(
        record,
        &selected.source_filename,
        declaration.vis.span(),
        fields.brace_token.span.close(),
    )?;
    let info = &record.inner["struct"];
    if record.visibility != "public"
        || !matches!(declaration.vis, Visibility::Public(_))
        || !declaration.generics.params.is_empty()
        || declaration.generics.where_clause.is_some()
        || info["kind"]["plain"]["has_stripped_fields"] != false
    {
        return Err("expected a plain unconditional public record".into());
    }
    let mut choices = BTreeMap::new();
    let mut identities = BTreeSet::from([selected.module_id.as_str(), selected.record.id.as_str()]);
    if identities.len() != 2 {
        return Err("duplicate declaration identity".into());
    }
    for choice in &selected.choices {
        let declarations: Vec<_> = syntax
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Enum(item) if item.ident == choice.name => Some(item),
                _ => None,
            })
            .collect();
        let [declaration] = declarations.as_slice() else {
            return Err("expected selected field declaration to be one source enum".into());
        };
        let documented = named_local(&children, &choice.name)?;
        match_span(
            documented,
            &selected.source_filename,
            declaration.vis.span(),
            declaration.brace_token.span.close(),
        )?;
        root_exports(&document, &root_items, module.id, documented, &root_source)?;
        if !matches!(declaration.vis, Visibility::Public(_))
            || documented.visibility != "public"
            || !declaration.generics.params.is_empty()
            || declaration.generics.where_clause.is_some()
            || documented.inner["enum"]["has_stripped_variants"] != false
            || declaration
                .variants
                .iter()
                .any(|variant| !matches!(variant.fields, syn::Fields::Unit))
            || !identities.insert(&choice.id)
            || choices.insert(documented.id, choice).is_some()
        {
            return Err("invalid or duplicate selected field declaration".into());
        }
    }
    let (traits, unknown_fields) = stock_traits(
        &syntax,
        declaration,
        record,
        &document,
        &selected.source_filename,
    )?;
    let field_ids = ids(&info["kind"]["plain"]["fields"])?;
    if field_ids.len() != fields.named.len() || fields.named.len() != selected.record.fields.len() {
        return Err("source, selection and compiler field counts disagree".into());
    }
    let mut recovered = Vec::new();
    let mut external_types = BTreeMap::new();
    for (field, id) in fields.named.iter().zip(field_ids) {
        let name = field
            .ident
            .as_ref()
            .ok_or("missing named source field")?
            .unraw()
            .to_string();
        let documented = document.item(id)?;
        match_span(
            documented,
            &selected.source_filename,
            field.vis.span(),
            field.ty.span(),
        )?;
        if documented.name.as_deref() != Some(&name)
            || documented.crate_id != 0
            || documented.visibility != "public"
            || !matches!(field.vis, Visibility::Public(_))
            || !field.attrs.iter().all(doc_attribute)
        {
            return Err("unsupported or mismatched public record field".into());
        }
        let field_id = selected
            .record
            .fields
            .get(&name)
            .ok_or("missing selected public field identity")?;
        if !identities.insert(field_id) {
            return Err("duplicate field identity".into());
        }
        let (value, link) = field_value(
            &field.ty,
            &documented.inner["struct_field"],
            &document,
            &choices,
            true,
        )?;
        if let Some(choice) = link {
            external_types.insert(choice.id.clone(), vec!["crate".into(), choice.name.clone()]);
        }
        recovered.push(Field {
            id: field_id.clone(),
            wire: name,
            value,
        });
    }
    if external_types.len() != choices.len() {
        return Err("unused selected external choice".into());
    }
    let graph = Graph {
        format: GRAPH.into(),
        modules: vec![Module {
            id: selected.module_id.clone(),
            aliases: vec![],
            records: vec![Record {
                id: selected.record.id.clone(),
                traits,
                fields: recovered,
                read_policy: Some(ReadPolicy {
                    shape: InputShape::ObjectOrOrderedSequence,
                    unknown_fields,
                }),
            }],
        }],
    };
    let overlay = Overlay {
        format: OVERLAY.into(),
        flatten_module: true,
        identifiers: BTreeMap::from([
            (selected.module_id, selected.module_name),
            (selected.record.id, selected.record.name),
        ]),
        external_types,
    };
    render(&graph, &overlay, true)?;
    let graph = encode(&graph)?;
    let overlay = encode(&overlay)?;
    let lock = encode(&json!({
        "format":"oddlang-serde-public-record-recovery-lock-v1",
        "selectionSha256":digest(&selection_bytes),
        "sourceSha256":selected.source_sha256, "crateRootSha256":selected.crate_root_sha256,
        "rustdocSha256":selected.rustdoc_sha256,
        "graphSha256":digest(&graph), "overlaySha256":digest(&overlay),
        "gap":"Independent downstream native callers must confirm the public export, traits and Serde wire behavior."
    }))?;
    emit(
        out,
        &[
            ("graph.json", &graph),
            ("rust.json", &overlay),
            ("recovery.lock", &lock),
        ],
    )
}
