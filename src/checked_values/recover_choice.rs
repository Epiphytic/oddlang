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

fn ordinary_derives(item: &syn::ItemEnum, debug: bool) -> Result<Equality> {
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
    if derives.len() != args.len()
        || !derives.contains("Clone")
        || !derives.contains("Copy")
        || derives.contains("Debug") != debug
        || derives
            .iter()
            .any(|name| !CAPABILITIES.contains(&name.as_str()) && name != "Debug")
    {
        return Err("unsupported derives on copyable plain choice".into());
    }
    match (derives.contains("PartialEq"), derives.contains("Eq")) {
        (true, true) => Ok(Equality::Total),
        (true, false) => Ok(Equality::Partial),
        (false, false) => Ok(Equality::None),
        (false, true) => Err("plain choice Eq requires PartialEq".into()),
    }
}

fn names(ident: &syn::Ident, expected: &str) -> bool {
    let ident = ident.to_string();
    ident.strip_prefix("r#").unwrap_or(&ident) == expected
}

fn additional_binding(file: &syn::File, name: &str) -> bool {
    use syn::visit::{self, Visit};

    struct Reference<'a> {
        name: &'a str,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Reference<'_> {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.found |= names(ident, self.name);
        }
    }

    struct Bindings<'a> {
        name: &'a str,
        found: bool,
    }
    impl Bindings<'_> {
        fn check_alias(&mut self, ty: &Type) {
            let mut alias = Reference {
                name: self.name,
                found: false,
            };
            alias.visit_type(ty);
            self.found |= alias.found;
        }
    }

    impl<'ast> Visit<'ast> for Bindings<'_> {
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let mut head = Reference {
                name: self.name,
                found: false,
            };
            head.visit_type(&item.self_ty);
            head.visit_generics(&item.generics);
            if let Some((_, trait_path, _)) = &item.trait_ {
                head.visit_path(trait_path);
            }
            self.found |= head.found;
            visit::visit_item_impl(self, item);
        }
        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            self.check_alias(&item.ty);
        }
        fn visit_impl_item_type(&mut self, item: &'ast syn::ImplItemType) {
            self.check_alias(&item.ty);
        }
        fn visit_trait_item_type(&mut self, item: &'ast syn::TraitItemType) {
            if let Some((_, ty)) = &item.default {
                self.check_alias(ty);
            }
        }
        fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
            self.found |= names(&item.ident, self.name);
        }
        fn visit_item_macro(&mut self, _item: &'ast syn::ItemMacro) {
            self.found = true;
        }
    }

    let mut scan = Bindings { name, found: false };
    scan.visit_file(file);
    scan.found
}

pub(crate) fn plain_choice_cases(item: &syn::ItemEnum, debug: bool) -> Result<Vec<String>> {
    if ordinary_derives(item, debug)? != Equality::Total {
        return Err("plain error reason requires total equality".into());
    }
    unit_cases(item)
}

fn unit_cases(item: &syn::ItemEnum) -> Result<Vec<String>> {
    if !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err("plain choice requires one public non-generic enum".into());
    }
    item.variants
        .iter()
        .map(|variant| {
            if !matches!(variant.fields, Fields::Unit)
                || variant.discriminant.is_some()
                || !variant.attrs.iter().all(|attr| attr.path().is_ident("doc"))
            {
                return Err(
                    "plain choice requires unconditional unit cases with implicit ordinals".into(),
                );
            }
            Ok(variant.ident.to_string())
        })
        .collect()
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
            "plain choice requires one public non-generic enum without aliases, additional impls or item macros"
                .into(),
        );
    }
    let mut cases = Vec::new();
    let mut names = BTreeMap::new();
    let type_id = format!("{}:type", selection.id);
    let equality = ordinary_derives(item, false)?;
    for label in unit_cases(item)? {
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
                equality,
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
