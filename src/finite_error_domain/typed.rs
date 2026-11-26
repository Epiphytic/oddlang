//! Recover a selected, equality-capable error family from pinned declarations.
use super::{
    recover::{Selection, check_alias, derive_error, docs},
    *,
};
use crate::root_data_scope as scope;
use syn::{Fields, Item, LitStr, Type, Visibility};

fn selected_scope(file: &syn::File, selected: &[&str], alias: &str) -> Result<()> {
    scope::ordinary_root(file)?;
    let mut inspected = file.clone();
    // Sibling item macro expansions remain a native name-resolution obligation.
    inspected.items.retain(|item| {
        !matches!(item, Item::Macro(_)) && !matches!(item, Item::Type(item) if item.ident == alias)
    });
    scope::bindings(&inspected, selected)?;
    let intrinsic = [
        "Debug",
        "Clone",
        "Copy",
        "Eq",
        "PartialEq",
        "thiserror",
        "std",
        "u8",
        "u16",
        "u32",
        "u64",
    ];
    if selected
        .iter()
        .chain([&alias])
        .any(|name| intrinsic.contains(name))
    {
        return Err("typed error selection shadows a required native declaration".into());
    }
    let reserved = selected
        .iter()
        .copied()
        .chain([alias])
        .chain(intrinsic)
        .collect::<Vec<_>>();
    for item in &file.items {
        let competing = match item {
            Item::Use(import) if scope::imports_reserved(&import.tree, None, &reserved) => {
                return Err("typed error declarations have unresolved import bindings".into());
            }
            Item::Enum(item) if !scope::name(&item.ident, selected) => Some(&item.ident),
            Item::Struct(item) => Some(&item.ident),
            Item::Union(item) => Some(&item.ident),
            Item::Trait(item) => Some(&item.ident),
            Item::TraitAlias(item) => Some(&item.ident),
            Item::Type(item) if item.ident != alias => Some(&item.ident),
            Item::Mod(item) => Some(&item.ident),
            Item::ExternCrate(item) => {
                Some(item.rename.as_ref().map_or(&item.ident, |(_, name)| name))
            }
            Item::Macro(item) => item.ident.as_ref(),
            _ => None,
        };
        if competing.is_some_and(|name| scope::name(name, &reserved)) {
            return Err("shadowing typed error declaration".into());
        }
    }
    Ok(())
}

fn product(
    fields: &syn::FieldsNamed,
    variant: &str,
    names: &mut BTreeMap<String, String>,
) -> Result<Payload> {
    let fields = fields
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            let Type::Path(ty) = &field.ty else {
                return Err("unsupported typed error payload field".into());
            };
            if !docs(&field.attrs)
                || !matches!(field.vis, Visibility::Inherited)
                || ty.qself.is_some()
                || name == "source"
            {
                return Err("unsupported typed error payload field".into());
            }
            let width = match ty.path.get_ident().map(ToString::to_string).as_deref() {
                Some("u8") => Width::U8,
                Some("u16") => Width::U16,
                Some("u32") => Width::U32,
                Some("u64") => Width::U64,
                _ => return Err("unsupported typed error payload field".into()),
            };
            let id = format!("{variant}:field:{name}");
            names.insert(id.clone(), name.to_string());
            Ok(ProductField { id, width })
        })
        .collect::<Result<_>>()?;
    Ok(Payload::Product { fields })
}

fn diagnostic(variant: &syn::Variant, payload: &Payload) -> Result<Message> {
    let attrs: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .collect();
    let [attribute] = attrs.as_slice() else {
        return Err("expected one typed error diagnostic".into());
    };
    if !attribute.path().is_ident("error") || variant.discriminant.is_some() {
        return Err("unsupported typed error attributes or discriminant".into());
    }
    let text = attribute
        .parse_args::<LitStr>()
        .map_err(|error| error.to_string())?
        .value();
    let braces = |text: &str| text.contains(['{', '}']);
    if !braces(&text) {
        return Ok(Message::Literal { text });
    }
    if matches!(payload, Payload::Choice { .. })
        && let Some((prefix, suffix)) = text.split_once("{0:?}")
        && !braces(prefix)
        && !braces(suffix)
    {
        return Ok(Message::Insertion {
            prefix: prefix.into(),
            suffix: suffix.into(),
        });
    }
    Err("unsupported typed error diagnostic interpolation".into())
}

pub(super) fn extract(source: &str, selection: Selection) -> Result<(Graph, Overlay)> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    let alias = selection
        .result
        .as_deref()
        .ok_or("typed error requires a result alias")?;
    let selected = std::iter::once(selection.error.as_str())
        .chain(selection.choices.iter().map(String::as_str))
        .collect::<Vec<_>>();
    if selected.iter().collect::<BTreeSet<_>>().len() != selected.len() {
        return Err("duplicate typed error selection".into());
    }
    selected_scope(&file, &selected, alias)?;

    let mut enums = BTreeMap::new();
    let mut result = None;
    for item in &file.items {
        match item {
            Item::Enum(item) if scope::name(&item.ident, &selected) => {
                if enums.insert(item.ident.to_string(), item).is_some() {
                    return Err("duplicate typed error declaration".into());
                }
            }
            Item::Type(item) if scope::name(&item.ident, &[alias]) => {
                if result.replace(item).is_some() {
                    return Err("duplicate typed result alias".into());
                }
            }
            _ => {}
        }
    }
    let error = enums
        .remove(&selection.error)
        .ok_or("missing typed error declaration")?;
    check_alias(result.ok_or("missing typed result alias")?, &error.ident)?;
    derive_error(error, false, true)?;

    let mut graph = Graph {
        format: "oddlang-finite-error-domain-v1".into(),
        id: selection.id.clone(),
        error: format!("{}:type:{}", selection.id, selection.error),
        result: Some(format!("{}:alias:{alias}", selection.id)),
        variants: BTreeMap::new(),
        categories: BTreeMap::new(),
        choices: BTreeMap::new(),
        equality: true,
    };
    let mut overlay = Overlay {
        format: "oddlang-finite-error-rust-v1".into(),
        names: BTreeMap::from([
            (graph.error.clone(), selection.error),
            (graph.result.clone().unwrap(), alias.to_owned()),
        ]),
        variant_order: Vec::new(),
        category_types: BTreeMap::new(),
    };
    let mut choices = BTreeMap::new();
    for name in selection.choices {
        let item = enums
            .get(&name)
            .ok_or("missing selected error payload choice")?;
        let id = format!("{}:choice:{name}", selection.id);
        let cases = crate::checked_values::plain_choice_cases(item, true)?
            .into_iter()
            .map(|text| {
                let id = format!("{id}:case:{text}");
                overlay.names.insert(id.clone(), text.clone());
                ChoiceCase { id, text }
            })
            .collect();
        graph.choices.insert(id.clone(), cases);
        overlay.names.insert(id.clone(), name.clone());
        choices.insert(name, id);
    }
    let mut variant_ids = selection.variant_ids;
    for variant in &error.variants {
        let name = variant.ident.to_string();
        let id = variant_ids
            .remove(&name)
            .unwrap_or_else(|| format!("{}:variant:{name}", selection.id));
        let payload = match &variant.fields {
            Fields::Unit => Payload::Unit,
            Fields::Named(fields) => product(fields, &id, &mut overlay.names)?,
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = fields.unnamed.first().unwrap();
                let Type::Path(ty) = &field.ty else {
                    return Err("expected local error payload choice".into());
                };
                if !docs(&field.attrs)
                    || ty.qself.is_some()
                    || !matches!(field.vis, Visibility::Inherited)
                {
                    return Err("unsupported typed error tuple payload".into());
                }
                let choice = ty
                    .path
                    .get_ident()
                    .and_then(|name| choices.get(&name.to_string()))
                    .ok_or("unknown selected error payload choice")?
                    .clone();
                Payload::Choice { choice }
            }
            _ => return Err("unsupported typed error payload".into()),
        };
        let message = diagnostic(variant, &payload)?;
        let entry = Variant {
            payload,
            message,
            source: false,
            from: false,
        };
        if graph.variants.insert(id.clone(), entry).is_some() {
            return Err("duplicate typed error variant identity".into());
        }
        overlay.names.insert(id.clone(), name);
        overlay.variant_order.push(id);
    }
    if !variant_ids.is_empty() {
        return Err("unused explicit error variant identity".into());
    }
    validate(&graph, &overlay)?;
    Ok((graph, overlay))
}
