//! Structural recovery for ordinary fieldless choices.
use super::*;
use syn::{Fields, Item, Meta, Type, UseTree, Visibility, punctuated::Punctuated, token::Comma};

const CAPABILITIES: [&str; 4] = ["Clone", "Copy", "Eq", "PartialEq"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    name: String,
    source_sha256: String,
}

fn reserved(name: &syn::Ident) -> bool {
    let name = name.to_string();
    CAPABILITIES.contains(&name.strip_prefix("r#").unwrap_or(&name))
}

fn shadow_import(tree: &UseTree, parent: Option<&syn::Ident>) -> bool {
    match tree {
        UseTree::Path(item) => shadow_import(&item.tree, Some(&item.ident)),
        UseTree::Name(item) if item.ident == "self" => parent.is_none_or(reserved),
        UseTree::Name(item) => reserved(&item.ident),
        UseTree::Rename(item) => reserved(&item.rename),
        UseTree::Group(item) => item.items.iter().any(|item| shadow_import(item, parent)),
        UseTree::Glob(_) => true,
    }
}

fn shadow(item: &Item) -> bool {
    match item {
        Item::Use(item) => shadow_import(&item.tree, None),
        Item::Mod(item) => {
            reserved(&item.ident) || item.attrs.iter().any(|a| a.path().is_ident("macro_use"))
        }
        Item::ExternCrate(item) => {
            reserved(item.rename.as_ref().map_or(&item.ident, |(_, alias)| alias))
                || item.attrs.iter().any(|a| a.path().is_ident("macro_use"))
        }
        Item::Enum(item) => reserved(&item.ident),
        Item::Struct(item) => reserved(&item.ident),
        Item::Type(item) => reserved(&item.ident),
        Item::Trait(item) => reserved(&item.ident),
        Item::TraitAlias(item) => reserved(&item.ident),
        Item::Macro(item) => item.ident.as_ref().is_some_and(reserved),
        _ => false,
    }
}

fn ordinary_derives(item: &syn::ItemEnum) -> Result<()> {
    let attrs: Vec<_> = item
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .collect();
    let [derive] = attrs.as_slice() else {
        return Err("plain choice requires one standard derive list".into());
    };
    if !derive.path().is_ident("derive") {
        return Err("unsupported plain choice attribute".into());
    }
    let args = derive
        .parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)
        .map_err(|e| e.to_string())?;
    let derives: BTreeSet<_> = args
        .iter()
        .filter_map(|meta| {
            meta.path()
                .get_ident()
                .filter(|_| matches!(meta, Meta::Path(_)))
        })
        .map(ToString::to_string)
        .collect();
    if args.len() != CAPABILITIES.len()
        || derives != CAPABILITIES.into_iter().map(str::to_owned).collect()
    {
        return Err("plain choice requires standard Clone, Copy, PartialEq and Eq derives".into());
    }
    Ok(())
}

fn names(ident: &syn::Ident, expected: &str) -> bool {
    let ident = ident.to_string();
    ident.strip_prefix("r#").unwrap_or(&ident) == expected
}

fn additional_binding(file: &syn::File, name: &str) -> bool {
    use syn::visit::{self, Visit};

    fn refers_to(ty: &Type, name: &str) -> bool {
        struct Reference<'a>(&'a str, bool);
        impl<'ast> Visit<'ast> for Reference<'_> {
            fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
                self.1 |= path
                    .path
                    .segments
                    .last()
                    .is_some_and(|part| names(&part.ident, self.0));
                visit::visit_type_path(self, path);
            }
        }
        let mut reference = Reference(name, false);
        reference.visit_type(ty);
        reference.1
    }

    struct Scan<'a>(&'a str, bool);
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            self.1 |= refers_to(&item.self_ty, self.0);
            visit::visit_item_impl(self, item);
        }
        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            self.1 |= refers_to(&item.ty, self.0);
        }
        fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
            self.1 |= names(&item.ident, self.0);
        }
    }

    let mut scan = Scan(name, false);
    scan.visit_file(file);
    scan.1
}

fn extract(source: &str, selection: &Selection) -> Result<(Graph, Binding)> {
    let file = syn::parse_file(source).map_err(|e| e.to_string())?;
    if !file.attrs.iter().all(|attr| attr.path().is_ident("doc")) || file.items.iter().any(shadow) {
        return Err("plain choice source may shadow standard derives".into());
    }
    let mut selected = file.items.iter().filter_map(|item| match item {
        Item::Enum(item) if item.ident == selection.name => Some(item),
        _ => None,
    });
    let item = selected.next().ok_or("missing selected plain choice")?;
    if selected.next().is_some()
        || !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
        || additional_binding(&file, &selection.name)
    {
        return Err(
            "plain choice requires one public non-generic enum without aliases or additional impls"
                .into(),
        );
    }
    ordinary_derives(item)?;
    let mut cases = Vec::new();
    let mut names = BTreeMap::new();
    let type_id = format!("{}:type", selection.id);
    for variant in &item.variants {
        if !matches!(variant.fields, Fields::Unit)
            || variant.discriminant.is_some()
            || !variant.attrs.iter().all(|attr| attr.path().is_ident("doc"))
        {
            return Err(
                "plain choice requires unconditional unit cases with implicit ordinals".into(),
            );
        }
        let label = variant.ident.to_string();
        let id = format!("{type_id}:{label}");
        names.insert(id.clone(), label);
        cases.push(id);
    }
    let graph = Graph {
        format: FORMAT.into(),
        id: selection.id.clone(),
        types: BTreeMap::from([(
            type_id.clone(),
            Definition::Choice {
                cases: cases.clone(),
            },
        )]),
    };
    let binding = Binding {
        format: "oddlang-checked-values-rust-v1".into(),
        types: BTreeMap::from([(
            type_id,
            Native {
                name: selection.name.clone(),
                order: cases,
                traits: vec![Derived::Copy],
                view: None,
                parse: None,
                shape: None,
                constructor: None,
                members: BTreeMap::new(),
            },
        )]),
        variants: names,
        errors: BTreeMap::new(),
    };
    graph.validate(&binding)?;
    Ok((graph, binding))
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let raw = fs::read(selection).map_err(|e| e.to_string())?;
    let selected: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let source = fs::read_to_string(source).map_err(|e| e.to_string())?;
    let source_digest = digest(source.as_bytes());
    if selected.format != "oddlang-plain-choice-selection-v1"
        || selected.source_sha256 != source_digest
    {
        return Err("plain choice selector version or source digest mismatch".into());
    }
    let (graph, binding) = extract(&source, &selected)?;
    let graph = encode(&graph)?;
    let binding = encode(&binding)?;
    let observation = encode(&json!({
        "format":"oddlang-plain-choice-observation-v1",
        "sourceSha256":source_digest,"selectionSha256":digest(&raw),
        "graphSha256":digest(&graph),"overlaySha256":digest(&binding),
        "gap":"native name resolution, trait capabilities and public module paths require downstream compilation"
    }))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, content) in [
        ("graph.json", graph),
        ("rust.json", binding),
        ("observation.json", observation),
    ] {
        fs::write(stage.path().join(name), content).map_err(|e| e.to_string())?;
    }
    publish(stage.path(), output)
}
