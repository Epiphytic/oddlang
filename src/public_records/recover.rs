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
    cargo_sha256: String,
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
    #[serde(rename = "scopeGlobs", default, deserialize_with = "unique_map")]
    scope_globs: BTreeMap<String, String>,
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

fn package_edition(lib: &Path, pin: &str) -> Result<String> {
    if !lib.ends_with("src/lib.rs") {
        return Err("public record recovery requires the package src/lib.rs".into());
    }
    let cargo = lib
        .parent()
        .and_then(Path::parent)
        .ok_or("missing public record package directory")?
        .join("Cargo.toml");
    if io(cargo.canonicalize())? != cargo {
        return Err("expected regular public record Cargo manifest".into());
    }
    let bytes = io(fs::read(&cargo))?;
    if digest(&bytes) != pin {
        return Err("public record Cargo manifest digest mismatch".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let manifest: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let library = manifest.get("lib");
    if library.and_then(|target| target.get("edition")).is_some() {
        return Err("public record recovery does not support a library edition override".into());
    }
    if let Some(path) = library.and_then(|target| target.get("path"))
        && path.as_str() != Some("src/lib.rs")
    {
        return Err("public record recovery requires the default package library path".into());
    }
    match manifest
        .get("package")
        .and_then(|package| package.get("edition"))
        .and_then(toml::Value::as_str)
    {
        Some(edition @ ("2018" | "2021" | "2024")) => Ok(edition.into()),
        _ => Err("public record recovery requires an explicit 2018 or newer Cargo edition".into()),
    }
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
            UseTree::Glob(_) => into.push(Import {
                name: "*".into(),
                path: prefix.clone(),
                declaration: use_item,
            }),
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

const PRELUDE_NAMES: [&str; 10] = [
    "bool",
    "u8",
    "u64",
    "String",
    "Vec",
    "std",
    "serde",
    "Clone",
    "Serialize",
    "derive",
];

fn item_type_binding(item: &Item) -> Option<(&syn::Ident, &Visibility)> {
    match item {
        Item::Enum(item) => Some((&item.ident, &item.vis)),
        Item::Struct(item) => Some((&item.ident, &item.vis)),
        Item::Type(item) => Some((&item.ident, &item.vis)),
        Item::Union(item) => Some((&item.ident, &item.vis)),
        Item::Mod(item) => Some((&item.ident, &item.vis)),
        Item::Trait(item) => Some((&item.ident, &item.vis)),
        Item::TraitAlias(item) => Some((&item.ident, &item.vis)),
        Item::ExternCrate(item) => Some((
            item.rename.as_ref().map_or(&item.ident, |(_, name)| name),
            &item.vis,
        )),
        _ => None,
    }
}

fn reserved(name: &syn::Ident) -> bool {
    PRELUDE_NAMES.iter().any(|n| *name == *n)
}

fn reject_item_expansion(file: &syn::File) -> Result<()> {
    if file.items.iter().any(|item| {
        matches!(
            item,
            Item::Macro(_) | Item::ForeignMod(_) | Item::Verbatim(_)
        )
    }) {
        return Err("unsupported expanded item in public record source scope".into());
    }
    Ok(())
}

fn reject_type_shadows(file: &syn::File, imports: &[Import<'_>]) -> Result<()> {
    for item in &file.items {
        if let Some((name, _)) = item_type_binding(item).filter(|(name, _)| reserved(name)) {
            return Err(format!("shadowed public record type or derive: {name}"));
        }
    }
    for item in imports
        .iter()
        .filter(|item| PRELUDE_NAMES.contains(&item.name.as_str()))
    {
        let canonical = match item.name.as_str() {
            "String" => &["std", "string", "String"][..],
            "Vec" => &["std", "vec", "Vec"][..],
            "Clone" => &["std", "clone", "Clone"][..],
            "Serialize" => &["serde", "Serialize"][..],
            _ => &[],
        };
        if item.path != canonical || !item.declaration.attrs.iter().all(is_doc) {
            return Err(format!(
                "shadowed public record type or derive: {}",
                item.name
            ));
        }
    }
    Ok(())
}

fn doc_module(file: &syn::File) -> Result<()> {
    if !file.attrs.iter().all(is_doc) {
        return Err("unsupported inner public record module attribute".into());
    }
    Ok(())
}

fn resolve_globs(
    directory: &Path,
    file: &syn::File,
    imports_in_scope: &[Import<'_>],
    pins: &BTreeMap<String, String>,
) -> Result<()> {
    let mut used = BTreeSet::new();
    for glob in imports_in_scope.iter().filter(|import| import.name == "*") {
        let child = match glob.path.as_slice() {
            [name] => name,
            [prefix, name] if prefix == "self" => name,
            _ => return Err("unsupported public record glob source".into()),
        };
        let pin = pins
            .get(child)
            .ok_or("unpinned public record glob source")?;
        if !symbol(child)
            || glob.declaration.leading_colon.is_some()
            || !glob.declaration.attrs.iter().all(is_doc)
        {
            return Err("unsupported public record glob binding".into());
        }
        if !used.insert(child) {
            continue;
        }
        let filename = format!("{child}.rs");
        let declarations = file
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Mod(found) if found.ident == child.as_str() => Some(found),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !matches!(declarations.as_slice(), [module] if module.content.is_none()
            && module.attrs.iter().filter(|attr| !is_doc(attr)).count() == 1
            && module.attrs.iter().any(|attr| matches!(&attr.meta,
                syn::Meta::NameValue(meta) if meta.path.is_ident("path")
                    && matches!(&meta.value, syn::Expr::Lit(literal)
                        if matches!(&literal.lit, syn::Lit::Str(path) if path.value() == filename)))))
        {
            return Err(format!("unresolved public record glob child: {child}"));
        }
        let path = directory.join(filename);
        if io(path.canonicalize())? != path {
            return Err(format!("non-local public record glob child: {child}"));
        }
        let scope = select_source(&path, pin)?;
        doc_module(&scope)?;
        reject_item_expansion(&scope)?;
        for item in &scope.items {
            if let Some((name, vis)) = item_type_binding(item)
                && !matches!(vis, Visibility::Inherited)
                && reserved(name)
            {
                return Err(format!("shadowed public record type through glob: {name}"));
            }
        }
        if imports(&scope).iter().any(|import| {
            !matches!(import.declaration.vis, Visibility::Inherited)
                && (import.name == "*" || PRELUDE_NAMES.contains(&import.name.as_str()))
        }) {
            return Err(format!(
                "unsupported public record glob re-export in child: {child}"
            ));
        }
    }
    if used.len() != pins.len() {
        return Err("unused public record glob source pin".into());
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
    let edition = package_edition(&lib, &selected.cargo_sha256)?;
    let directory = lib.parent().ok_or("missing Rust module directory")?;
    let crate_source = select_source(&lib, &selected.lib_sha256)?;
    doc_module(&crate_source)?;
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
        doc_module(&rust)?;
        reject_item_expansion(&rust)?;
        let imports = imports(&rust);
        if part.records.is_empty() {
            if !part.scope_globs.is_empty() {
                return Err("unused public record glob source pin".into());
            }
        } else {
            reject_type_shadows(&rust, &imports)?;
            resolve_globs(directory, &rust, &imports, &part.scope_globs)?;
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
    let scope_globs: BTreeMap<_, _> = selected
        .modules
        .iter()
        .filter(|module| !module.scope_globs.is_empty())
        .map(|module| (&module.name, &module.scope_globs))
        .collect();
    let lock = encode(&serde_json::json!({
        "format":"oddlang-public-record-recovery-lock-v1",
        "selectionSha256":digest(&selection_bytes),
        "crateSourceSha256":selected.lib_sha256,
        "cargoManifestSha256":selected.cargo_sha256,
        "rustEdition":edition,
        "moduleSourceSha256":modules,
        "globSourceSha256":scope_globs,
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
