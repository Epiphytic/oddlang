//! Recover the existing shared-declarations string choice profile from Rust.
use super::*;
use crate::{
    checked_values::additional_binding,
    native_package::publish,
    public_records::{imports, item_type_binding},
    wire_schema::encode,
};
use serde_json::json;
use syn::{
    Fields, Item, ItemEnum, Meta, PathArguments, Token, Visibility, ext::IdentExt,
    punctuated::Punctuated,
};

const STOCK: [&str; 5] = ["Clone", "Copy", "Debug", "PartialEq", "Eq"];
const SERDE: [&str; 2] = ["Deserialize", "Serialize"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    enum_id: String,
    name: String,
    source_sha256: String,
    #[serde(deserialize_with = "unique_map")]
    variants: BTreeMap<String, String>,
}

fn doc(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("doc")
}

fn derive_bindings(file: &syn::File) -> Result<BTreeMap<String, Vec<String>>> {
    let imports = imports(file);
    if !file.attrs.iter().all(doc)
        || imports.iter().any(|import| import.name == "*")
        || file.items.iter().any(|item| {
            let attrs = match item {
                Item::Mod(item) => &item.attrs,
                Item::ExternCrate(item) => &item.attrs,
                Item::Use(item) => &item.attrs,
                _ => return false,
            };
            attrs.iter().any(|attr| attr.path().is_ident("macro_use"))
        })
    {
        return Err("unsupported derive scope in Serde string choice".into());
    }
    let mut bound = BTreeMap::<String, Vec<String>>::new();
    for import in imports {
        let canonical = match import.path.as_slice() {
            [root, name]
                if root == "serde"
                    && SERDE.contains(&name.as_str())
                    && import.declaration.attrs.iter().all(doc) =>
            {
                name.clone()
            }
            _ => String::new(),
        };
        bound.entry(import.name).or_default().push(canonical);
    }
    for item in &file.items {
        let name = match item {
            Item::Macro(item) => item.ident.as_ref(),
            _ => item_type_binding(item).map(|(name, _)| name),
        };
        if let Some(name) = name {
            bound
                .entry(name.unraw().to_string())
                .or_default()
                .push(String::new());
        }
    }
    if bound.contains_key("serde")
        || bound.contains_key("derive")
        || STOCK.iter().any(|name| bound.contains_key(*name))
    {
        return Err("shadowed Serde or stock derive binding".into());
    }
    Ok(bound)
}

fn capabilities(item: &ItemEnum, bindings: &BTreeMap<String, Vec<String>>) -> Result<bool> {
    let attrs: Vec<_> = item.attrs.iter().filter(|a| !doc(a)).collect();
    let [attr] = attrs.as_slice() else {
        return Err("unsupported enum attribute on Serde string choice".into());
    };
    if !attr.path().is_ident("derive") {
        return Err("unsupported enum attribute on Serde string choice".into());
    }
    let parsed = attr
        .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
        .map_err(|e| e.to_string())?;
    let mut traits = BTreeSet::new();
    for path in parsed {
        if path
            .segments
            .iter()
            .any(|s| !matches!(s.arguments, PathArguments::None))
        {
            return Err("unsupported derive path on Serde string choice".into());
        }
        let names: Vec<_> = path
            .segments
            .iter()
            .map(|s| s.ident.unraw().to_string())
            .collect();
        let canonical = match names.as_slice() {
            [name] if path.leading_colon.is_none() && STOCK.contains(&name.as_str()) => name,
            [root, name] if root == "serde" && SERDE.contains(&name.as_str()) => name,
            [name] if path.leading_colon.is_none() => match bindings.get(name).map(Vec::as_slice) {
                Some([value]) if SERDE.contains(&value.as_str()) => value,
                Some(values) if values.len() > 1 => {
                    return Err(format!("ambiguous derive binding: {name}"));
                }
                _ => return Err(format!("unresolved Serde derive: {name}")),
            },
            _ => return Err("unsupported derive path on Serde string choice".into()),
        };
        if !traits.insert(canonical.clone()) {
            return Err(format!("duplicate derive capability: {canonical}"));
        }
    }
    let copy = traits.remove("Copy");
    if traits.iter().map(String::as_str).collect::<BTreeSet<_>>()
        != BTreeSet::from([
            "Clone",
            "Debug",
            "Deserialize",
            "Eq",
            "PartialEq",
            "Serialize",
        ])
    {
        return Err(
            "Serde string choice requires the exact shared-declarations derive capabilities".into(),
        );
    }
    Ok(copy)
}

fn wire(variant: &syn::Variant) -> Result<String> {
    if !matches!(variant.fields, Fields::Unit) || variant.discriminant.is_some() {
        return Err("expected unconditional unit variant with implicit ordinal".into());
    }
    let attrs: Vec<_> = variant.attrs.iter().filter(|a| !doc(a)).collect();
    match attrs.as_slice() {
        [] => Ok(variant.ident.unraw().to_string()),
        [attr] if attr.path().is_ident("serde") => {
            let meta = attr
                .parse_args::<Meta>()
                .map_err(|_| "unsupported Serde variant attribute")?;
            if let Meta::NameValue(value) = meta
                && value.path.is_ident("rename")
                && let syn::Expr::Lit(expr) = value.value
                && expr.attrs.is_empty()
                && let syn::Lit::Str(name) = expr.lit
            {
                return Ok(name.value());
            }
            Err("unsupported Serde variant attribute".into())
        }
        _ => Err("unsupported Serde variant attribute".into()),
    }
}

fn recover_bytes(source: &[u8], selection: &[u8]) -> Result<[Vec<u8>; 3]> {
    let spec: Selection = serde_json::from_slice(selection).map_err(|e| e.to_string())?;
    if spec.format != "oddlang-rust-serde-string-choice-selection-v1" {
        return Err("invalid Serde string choice selection format".into());
    }
    let source_hash = digest(source);
    if source_hash != spec.source_sha256 {
        return Err("Serde string choice source digest mismatch".into());
    }
    let text = std::str::from_utf8(source).map_err(|e| e.to_string())?;
    let file = syn::parse_file(text).map_err(|e| e.to_string())?;
    let bindings = derive_bindings(&file)?;
    let choices: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(item) if item.ident.unraw() == spec.name.as_str() => Some(item),
            _ => None,
        })
        .collect();
    let [item] = choices.as_slice() else {
        return Err("expected exactly one selected Rust enum".into());
    };
    if !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err("selected Rust enum must be public and non-generic".into());
    }
    if additional_binding(&file, &spec.name, false)
        || bindings
            .get(&spec.name)
            .is_some_and(|names| names.len() != 1)
    {
        return Err("selected enum has an additional binding".into());
    }
    let copy = capabilities(item, &bindings)?;
    let mut names = BTreeMap::from([(spec.enum_id.clone(), spec.name)]);
    let mut variants = Vec::new();
    for variant in &item.variants {
        let name = variant.ident.unraw().to_string();
        let id = spec
            .variants
            .get(&name)
            .ok_or_else(|| format!("missing variant identity: {name}"))?;
        if names.insert(id.clone(), name).is_some() {
            return Err(format!("reused variant identity: {id}"));
        }
        variants.push(WireVariant {
            id: id.clone(),
            wire: wire(variant)?,
        });
    }
    if variants.len() != spec.variants.len() {
        return Err("unused selected variant identity".into());
    }
    let order: Vec<_> = variants.iter().map(|v| v.id.clone()).collect();
    let choice = StringEnum {
        id: spec.enum_id.clone(),
        copy,
        numbers: None,
        variants,
    };
    let graph = json!({
        "format":"oddlang-shared-declarations-v1", "id":spec.id, "stringEnums":[choice],
    });
    let overlay = json!({
        "format":"oddlang-shared-rust-v1", "names":names, "constantTypes":{},
        "variantOrder":{(&spec.enum_id):order},
    });
    let g: Graph = serde_json::from_value(graph.clone()).map_err(|e| e.to_string())?;
    let b: Binding = serde_json::from_value(overlay.clone()).map_err(|e| e.to_string())?;
    g.validate(&b)?;
    let graph = encode(&graph)?;
    let overlay = encode(&overlay)?;
    let lock = encode(&json!({
        "format":"oddlang-rust-serde-string-choice-recovery-v1",
        "sourceSha256":source_hash, "selectionSha256":digest(selection),
        "graphSha256":digest(&graph), "overlaySha256":digest(&overlay),
        "qualification":"Syntax candidate. Rust derive resolution, sibling macro expansion and public exports require independent native confirmation.",
    }))?;
    Ok([graph, overlay, lock])
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let read = |path: &Path| fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let artifacts = recover_bytes(&read(source)?, &read(selection)?)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, data) in ["graph.json", "rust.json", "recovery.lock"]
        .into_iter()
        .zip(artifacts)
    {
        fs::write(stage.path().join(name), data).map_err(|e| e.to_string())?;
    }
    publish(stage.path(), output)
}
