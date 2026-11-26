use super::*;
use syn::{
    Attribute, GenericArgument, Item, ItemStruct, ItemUse, PathArguments, Token, Type, UseTree,
    Visibility, punctuated::Punctuated,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    lib_sha256: String,
    modules: Vec<SelectedModule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedModule {
    id: String,
    name: String,
    sha256: String,
    #[serde(default, deserialize_with = "unique_map")]
    records: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "unique_map")]
    aliases: BTreeMap<String, String>,
}

struct Import<'a> {
    name: String,
    path: Vec<String>,
    declaration: &'a ItemUse,
}

fn select_source(path: &Path, pin: &str) -> Result<syn::File> {
    let bytes = io(fs::read(path))?;
    if digest(&bytes) != pin {
        return Err(format!(
            "public record source digest mismatch: {}",
            path.display()
        ));
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    syn::parse_file(text).map_err(|e| format!("invalid Rust source {}: {e}", path.display()))
}

fn module_file(root: &Path, lib: &syn::File, name: &str) -> Result<std::path::PathBuf> {
    if !symbol(name) {
        return Err(format!("invalid selected public module: {name}"));
    }
    let matches = lib
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(module) if module.ident == name => Some(module),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !matches!(matches.as_slice(), [module] if
        matches!(module.vis, Visibility::Public(_))
        && module.content.is_none()
        && module.attrs.iter().all(is_doc))
    {
        return Err(format!(
            "expected one unconditional public module declaration: {name}"
        ));
    }
    let path = root.join(format!("{name}.rs"));
    if root.join(name).join("mod.rs").exists() {
        return Err(format!("unsupported alternate Rust module path for {name}"));
    }
    let resolved = io(path.canonicalize())?;
    if resolved != path || !resolved.is_file() {
        return Err(format!(
            "expected regular sibling public module source for {name}"
        ));
    }
    Ok(resolved)
}

fn is_doc(attr: &Attribute) -> bool {
    attr.path().is_ident("doc")
}

fn imports(file: &syn::File) -> Vec<Import<'_>> {
    fn flatten<'a>(
        use_item: &'a ItemUse,
        tree: &UseTree,
        prefix: &mut Vec<String>,
        into: &mut Vec<Import<'a>>,
    ) {
        match tree {
            UseTree::Path(node) => {
                prefix.push(node.ident.to_string());
                flatten(use_item, &node.tree, prefix, into);
                prefix.pop();
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    flatten(use_item, tree, prefix, into);
                }
            }
            UseTree::Name(node) => {
                let name = node.ident.to_string();
                let mut path = prefix.clone();
                path.push(name.clone());
                into.push(Import {
                    name,
                    path,
                    declaration: use_item,
                });
            }
            UseTree::Rename(node) => {
                let mut path = prefix.clone();
                path.push(node.ident.to_string());
                into.push(Import {
                    name: node.rename.to_string(),
                    path,
                    declaration: use_item,
                });
            }
            UseTree::Glob(_) => {}
        }
    }

    let mut found = Vec::new();
    for item in &file.items {
        if let Item::Use(use_item) = item {
            flatten(use_item, &use_item.tree, &mut Vec::new(), &mut found);
        }
    }
    found
}

fn reject_type_shadows(file: &syn::File, imports: &[Import<'_>]) -> Result<()> {
    let names = [
        "bool", "u8", "u64", "String", "Vec", "std", "serde", "Clone",
    ];
    for item in &file.items {
        let declared = match item {
            Item::Enum(item) => Some(&item.ident),
            Item::Struct(item) => Some(&item.ident),
            Item::Type(item) => Some(&item.ident),
            Item::Union(item) => Some(&item.ident),
            Item::Mod(item) => Some(&item.ident),
            Item::Trait(item) => Some(&item.ident),
            Item::ExternCrate(item) => {
                Some(item.rename.as_ref().map_or(&item.ident, |(_, name)| name))
            }
            _ => None,
        };
        if let Some(name) = declared.filter(|name| names.iter().any(|n| **name == *n)) {
            return Err(format!("shadowed public record type or derive: {name}"));
        }
    }
    for item in imports
        .iter()
        .filter(|item| names.contains(&item.name.as_str()))
    {
        let canonical = match item.name.as_str() {
            "String" => &["std", "string", "String"][..],
            "Vec" => &["std", "vec", "Vec"][..],
            "Clone" => &["std", "clone", "Clone"][..],
            _ => &[],
        };
        if item.path != canonical || !item.declaration.attrs.iter().all(is_doc) {
            return Err(format!(
                "shadowed public record type or derive: {}",
                item.name
            ));
        }
    }
    if file
        .attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("no_implicit_prelude"))
    {
        return Err("public record source requires the implicit prelude".into());
    }
    Ok(())
}

fn path_names(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|s| s.ident.to_string()).collect()
}

fn path_is(path: &syn::Path, names: &[&str]) -> bool {
    path_names(path) == names && path.segments.iter().all(|s| s.arguments.is_empty())
}

fn derive_traits(attrs: &[Attribute], imports: &[Import<'_>]) -> Result<Vec<Trait>> {
    let local_serde = imports
        .iter()
        .filter(|item| item.name == "Serialize")
        .collect::<Vec<_>>();
    let source_serde = matches!(local_serde.as_slice(), [item]
        if item.path == ["serde", "Serialize"] && item.declaration.attrs.iter().all(is_doc));
    let mut traits = Vec::new();
    for attr in attrs.iter().filter(|a| !is_doc(a)) {
        if !attr.path().is_ident("derive") {
            return Err("unsupported record attribute".into());
        }
        let names = attr
            .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
            .map_err(|e| e.to_string())?;
        for path in &names {
            let capability = if path.is_ident("Clone") || path_is(path, &["std", "clone", "Clone"])
            {
                Trait::Clone
            } else if (path.is_ident("Serialize") && source_serde)
                || path_is(path, &["serde", "Serialize"])
            {
                Trait::Serialize
            } else {
                return Err("unsupported public record derive".into());
            };
            traits.push(capability);
        }
    }
    Ok(traits)
}

fn field_type(ty: &Type) -> Result<Value> {
    let unsupported = || "unsupported public record field type".to_owned();
    let Type::Path(node) = ty else {
        return Err(unsupported());
    };
    if node.qself.is_some() {
        return Err(unsupported());
    }
    let path = &node.path;
    if path.is_ident("bool") {
        return Ok(Value::Bool);
    }
    if path.is_ident("u64") {
        return Ok(Value::U64);
    }
    if path.is_ident("String") || path_is(path, &["std", "string", "String"]) {
        return Ok(Value::Text);
    }
    if path.segments.len() == 1 || path_names(path) == ["std", "vec", "Vec"] {
        let last = path.segments.last().ok_or_else(unsupported)?;
        if last.ident == "Vec"
            && path
                .segments
                .iter()
                .rev()
                .skip(1)
                .all(|s| s.arguments.is_empty())
            && let PathArguments::AngleBracketed(parameters) = &last.arguments
            && matches!(parameters.args.iter().collect::<Vec<_>>().as_slice(),
                [GenericArgument::Type(Type::Path(element))] if element.qself.is_none() && element.path.is_ident("u8"))
        {
            return Ok(Value::Octets);
        }
    }
    Err(unsupported())
}

fn record(item: &ItemStruct, identity: &str, imports: &[Import<'_>]) -> Result<Record> {
    if !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(format!(
            "expected an unconditional public record: {}",
            item.ident
        ));
    }
    let syn::Fields::Named(fields) = &item.fields else {
        return Err(format!(
            "expected named public record fields: {}",
            item.ident
        ));
    };
    let mut recovered = Vec::new();
    for field in &fields.named {
        if !field.attrs.iter().all(is_doc) || !matches!(field.vis, Visibility::Public(_)) {
            return Err(format!(
                "unsupported non-public or attributed field in {}",
                item.ident
            ));
        }
        let wire = field.ident.as_ref().unwrap().to_string();
        recovered.push(Field {
            id: format!("{identity}/field/{wire}"),
            wire,
            value: field_type(&field.ty)?,
        });
    }
    Ok(Record {
        id: identity.into(),
        traits: derive_traits(&item.attrs, imports)?,
        fields: recovered,
    })
}

pub fn recover(lib: &Path, selection: &Path, out: &Path) -> Result<()> {
    let selection_bytes = io(fs::read(selection))?;
    let selected: Selection =
        serde_json::from_slice(&selection_bytes).map_err(|e| e.to_string())?;
    if selected.format != "oddlang-rust-public-record-selection-v1" || selected.modules.is_empty() {
        return Err("invalid public record source selection".into());
    }
    let lib = io(lib.canonicalize())?;
    let directory = lib.parent().ok_or("missing Rust module directory")?;
    let crate_source = select_source(&lib, &selected.lib_sha256)?;
    if crate_source.attrs.iter().any(|attribute| {
        attribute.path().is_ident("no_std") || attribute.path().is_ident("no_implicit_prelude")
    }) {
        return Err("public record source requires the standard implicit prelude".into());
    }
    let mut bindings = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut graph = Graph {
        format: GRAPH.into(),
        modules: Vec::new(),
    };
    for part in &selected.modules {
        let path = module_file(directory, &crate_source, &part.name)?;
        let rust = select_source(&path, &part.sha256)?;
        let imports = imports(&rust);
        if !part.records.is_empty() {
            reject_type_shadows(&rust, &imports)?;
        }
        let mut module = Module {
            id: part.id.clone(),
            records: Vec::new(),
            aliases: Vec::new(),
        };
        bind(&mut bindings, &module.id, &part.name)?;
        for item in &rust.items {
            if let Item::Struct(candidate) = item {
                let name = candidate.ident.to_string();
                if let Some(identity) = part.records.get(&name) {
                    bind(&mut bindings, identity, &name)?;
                    module.records.push(record(candidate, identity, &imports)?);
                    identities.insert((part.name.clone(), name), identity.clone());
                }
            }
        }
        if module.records.len() != part.records.len() {
            return Err(format!(
                "missing public record in selected module {}",
                part.name
            ));
        }
        graph.modules.push(module);
        if sources.insert(&part.name, rust).is_some() {
            return Err(format!("duplicate selected module {}", part.name));
        }
    }
    for (module, part) in graph.modules.iter_mut().zip(&selected.modules) {
        let imports = imports(&sources[&part.name]);
        for (name, id) in &part.aliases {
            let exports = imports
                .iter()
                .filter(|item| {
                    item.name == *name && matches!(item.declaration.vis, Visibility::Public(_))
                })
                .collect::<Vec<_>>();
            let target = match exports.as_slice() {
                [item]
                    if item.declaration.attrs.iter().all(is_doc)
                        && item.declaration.leading_colon.is_none() =>
                {
                    match item.path.as_slice() {
                        [root, owner, record] if root == "crate" => {
                            identities.get(&(owner.clone(), record.clone()))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
            .ok_or_else(|| format!("unresolved public record re-export: {}::{name}", part.name))?;
            bind(&mut bindings, id, name)?;
            module.aliases.push(Alias {
                id: id.clone(),
                target: target.clone(),
            });
        }
    }
    let overlay = Overlay {
        format: OVERLAY.into(),
        identifiers: bindings,
    };
    render(&graph, &overlay, true)?;
    let graph = encode(&graph)?;
    let overlay = encode(&overlay)?;
    let modules: BTreeMap<_, _> = selected
        .modules
        .iter()
        .map(|module| (&module.name, &module.sha256))
        .collect();
    let lock = encode(&serde_json::json!({
        "format":"oddlang-public-record-recovery-lock-v1",
        "selectionSha256":digest(&selection_bytes),
        "crateSourceSha256":selected.lib_sha256,
        "moduleSourceSha256":modules,
        "graphSha256":digest(&graph),
        "overlaySha256":digest(&overlay),
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

fn bind(bindings: &mut BTreeMap<String, String>, id: &str, name: &str) -> Result<()> {
    if bindings.insert(id.into(), name.into()).is_some() {
        return Err(format!("duplicate public record identity: {id}"));
    }
    Ok(())
}
