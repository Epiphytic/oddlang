//! Recover the Serde data contract of a nested restriction list from pinned Rust source.
use crate::source_observation::{publish, read};
use crate::wire_schema::{digest, encode, iri, json};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::{Attribute, Field, Item, ItemFn, ItemStruct, Meta, Token, Type, punctuated::Punctuated};

type Result<T> = std::result::Result<T, String>;
type Names = BTreeMap<String, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    source_sha256: String,
    records: Names,
    fields: Names,
}

fn roles(names: &Names, required: &[&str]) -> Result<()> {
    if names.len() != required.len() || required.iter().any(|role| !names.contains_key(*role)) {
        return Err(format!("expected selection roles {required:?}"));
    }
    for name in names.values() {
        syn::parse_str::<syn::Ident>(name).map_err(|e| e.to_string())?;
        if name.starts_with("r#") {
            return Err(format!("unsupported raw identifier {name}"));
        }
    }
    Ok(())
}

fn metas(attr: &Attribute) -> Result<Vec<Meta>> {
    attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        .map(|items| items.into_iter().collect())
        .map_err(|e| e.to_string())
}

fn record<'a>(source: &'a syn::File, name: &str, needs_default: bool) -> Result<&'a ItemStruct> {
    let matches: Vec<_> = source
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(record) if record.ident == name => Some(record),
            _ => None,
        })
        .collect();
    let [record] = matches.as_slice() else {
        return Err(format!("expected one struct {name}"));
    };
    let mut derives = Vec::new();
    let mut closed = false;
    for attr in &record.attrs {
        if attr.path().is_ident("derive") {
            for meta in metas(attr)? {
                let Meta::Path(path) = meta else {
                    return Err(format!("unsupported derive on {name}"));
                };
                derives.push(
                    path.get_ident()
                        .ok_or("expected unqualified derive")?
                        .to_string(),
                );
            }
        } else if attr.path().is_ident("serde")
            && !closed
            && metas(attr)? == [syn::parse_quote!(deny_unknown_fields)]
        {
            closed = true;
        } else if !attr.path().is_ident("doc") {
            return Err(format!("unsupported struct attribute on {name}"));
        }
    }
    derives.sort();
    let expected = if needs_default {
        vec!["Default", "Deserialize"]
    } else {
        vec!["Deserialize"]
    };
    if !closed
        || derives != expected
        || !record.generics.params.is_empty()
        || record.generics.where_clause.is_some()
        || !matches!(record.fields, syn::Fields::Named(_))
    {
        return Err(format!("unsupported Serde record {name}"));
    }
    Ok(record)
}

#[derive(Default)]
struct FieldAttrs {
    default: bool,
    adapter: Option<String>,
}

fn field_attrs(field: &Field) -> Result<FieldAttrs> {
    let mut result = FieldAttrs::default();
    for attr in &field.attrs {
        if attr.path().is_ident("doc") {
            continue;
        }
        if !attr.path().is_ident("serde") {
            return Err("unsupported field attribute".into());
        }
        for meta in metas(attr)? {
            match meta {
                Meta::Path(p) if p.is_ident("default") && !result.default => result.default = true,
                Meta::NameValue(n)
                    if n.path.is_ident("deserialize_with") && result.adapter.is_none() =>
                {
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(name),
                        ..
                    }) = n.value
                    else {
                        return Err("deserialize_with must name a function".into());
                    };
                    result.adapter = Some(name.value());
                }
                _ => return Err("unsupported or duplicate serde field option".into()),
            }
        }
    }
    Ok(result)
}

fn check_field(
    record: &ItemStruct,
    name: &str,
    ty: &str,
    default: bool,
    adapter: bool,
) -> Result<Option<String>> {
    let mut matches = record
        .fields
        .iter()
        .filter(|field| field.ident.as_ref().is_some_and(|id| id == name));
    let field = matches
        .next()
        .ok_or_else(|| format!("missing field {}.{name}", record.ident))?;
    let attrs = field_attrs(field)?;
    if matches.next().is_some()
        || field.ty != syn::parse_str::<Type>(ty).map_err(|e| e.to_string())?
        || attrs.default != default
        || attrs.adapter.is_some() != adapter
    {
        return Err(format!(
            "unsupported type or Serde behavior for {}.{name}",
            record.ident
        ));
    }
    Ok(attrs.adapter)
}

fn verify_adapter(file: &syn::File, name: &str) -> Result<()> {
    let ident = syn::parse_str::<syn::Ident>(name).map_err(|e| e.to_string())?;
    let expected: ItemFn = syn::parse_quote! {
        fn #ident<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
        where D: Deserializer<'de>, T: Deserialize<'de>, {
            T::deserialize(d).map(Some)
        }
    };
    let mut matches = file.items.iter().filter_map(|item| match item {
        Item::Fn(f) if f.sig.ident == ident => Some(f),
        _ => None,
    });
    let function = matches.next().ok_or("missing non-null adapter")?;
    if matches.next().is_some()
        || function.sig != expected.sig
        || function.block != expected.block
        || function
            .attrs
            .iter()
            .any(|attr| !attr.path().is_ident("doc"))
    {
        return Err("unsupported non-null adapter function".into());
    }
    Ok(())
}

// The profile recognizes unqualified prelude types and the two imported Serde traits.
// Resolve these limited names explicitly before interpreting their spelling.
fn check_scope(file: &syn::File, records: &Names, adapter: &str) -> Result<()> {
    use syn::{UseTree, ext::IdentExt};
    let mut reserved = vec![
        "String",
        "Vec",
        "Option",
        "Some",
        "Default",
        "Deserialize",
        "Deserializer",
        "std",
        "serde",
        "u32",
    ];
    if records
        .values()
        .any(|name| reserved.contains(&name.as_str()))
        || reserved.contains(&adapter)
    {
        return Err("selected name shadows a required binding".into());
    }
    reserved.extend(records.values().map(String::as_str));
    reserved.push(adapter);
    let mut traits = BTreeSet::new();
    fn imports(
        tree: &UseTree,
        path: &mut Vec<String>,
        names: &[&str],
        traits: &mut BTreeSet<String>,
    ) -> Result<()> {
        match tree {
            UseTree::Path(p) => {
                path.push(p.ident.unraw().to_string());
                imports(&p.tree, path, names, traits)?;
                path.pop();
            }
            UseTree::Group(group) => {
                for child in &group.items {
                    imports(child, path, names, traits)?;
                }
            }
            UseTree::Name(n) => {
                let name = n.ident.unraw().to_string();
                let bound = if name == "self" {
                    path.last().ok_or("unsupported self import")?
                } else {
                    &name
                };
                if names.contains(&bound.as_str()) {
                    if path.as_slice() != ["serde"]
                        || !["Deserialize", "Deserializer"].contains(&name.as_str())
                        || !traits.insert(name)
                    {
                        return Err("ambiguous restriction-list import".into());
                    }
                }
            }
            UseTree::Rename(n) if !crate::root_data_scope::name(&n.rename, names) => {}
            _ => return Err("unsupported shadowing or glob import".into()),
        }
        Ok(())
    }
    for item in &file.items {
        let name = match item {
            Item::Use(u) => {
                let mut seen = BTreeSet::new();
                imports(&u.tree, &mut Vec::new(), &reserved, &mut seen)?;
                if !seen.is_empty() && u.attrs.iter().any(|a| !a.path().is_ident("doc")) {
                    return Err("conditional Serde import".into());
                }
                for name in seen {
                    if !traits.insert(name) {
                        return Err("duplicate Serde import".into());
                    }
                }
                continue;
            }
            Item::Struct(s) if records.values().any(|n| s.ident == n.as_str()) => continue,
            Item::Fn(f) if f.sig.ident == adapter => continue,
            Item::Struct(s) => &s.ident,
            Item::Fn(f) => &f.sig.ident,
            Item::Mod(m)
                if !m
                    .attrs
                    .iter()
                    .any(|a| a.path().is_ident("macro_use") || a.path().is_ident("cfg_attr")) =>
            {
                &m.ident
            }
            Item::Enum(e) => &e.ident,
            Item::Type(t) => &t.ident,
            Item::Const(c) => &c.ident,
            Item::Static(s) => &s.ident,
            Item::Trait(t) => &t.ident,
            Item::Union(u) => &u.ident,
            Item::Impl(_) => continue,
            _ => return Err("unsupported item in restriction-list scope".into()),
        };
        if crate::root_data_scope::name(name, &reserved) {
            return Err("shadowed restriction-list binding".into());
        }
    }
    if traits.len() != 2 {
        return Err("missing Serde trait import".into());
    }
    Ok(())
}

fn graph(bytes: &[u8], selection: &Selection) -> Result<Value> {
    if selection.format != "oddlang-restriction-lists-selection-v1"
        || digest(bytes) != selection.source_sha256
    {
        return Err("unsupported selection or mismatched source digest".into());
    }
    iri(&selection.id)?;
    let r = &selection.records;
    let f = &selection.fields;
    roles(r, &["config", "policy", "entry", "restrictions"])?;
    roles(
        f,
        &[
            "policy",
            "version",
            "defaults",
            "entries",
            "principal",
            "restrictions",
            "allow",
            "deny",
        ],
    )?;
    if r.values().collect::<BTreeSet<_>>().len() != r.len() {
        return Err("each record role needs a distinct struct".into());
    }
    let file = syn::parse_file(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    crate::root_data_scope::ordinary_root(&file)?;
    let config = record(&file, &r["config"], false)?;
    let policy = record(&file, &r["policy"], false)?;
    let entry = record(&file, &r["entry"], false)?;
    let restrictions = record(&file, &r["restrictions"], true)?;
    let mut sequence_fields = BTreeMap::new();
    for (name, record, order) in [
        ("policy", policy, &["version", "defaults", "entries"][..]),
        ("entry", entry, &["principal", "restrictions"][..]),
        ("restrictions", restrictions, &["allow", "deny"][..]),
    ] {
        let source_names = record
            .fields
            .iter()
            .map(|field| field.ident.as_ref().map(ToString::to_string));
        if !source_names.eq(order.iter().map(|role| Some(f[*role].clone()))) {
            return Err(format!(
                "unsupported positional field order for {}",
                record.ident
            ));
        }
        sequence_fields.insert(name, order);
    }
    // Serde flatten, aliases, or renames on siblings could change the root policy key.
    for sibling in &config.fields {
        field_attrs(sibling)?;
    }
    check_field(config, &f["policy"], &r["policy"], false, false)?;
    check_field(policy, &f["version"], "u32", false, false)?;
    check_field(policy, &f["defaults"], &r["restrictions"], true, false)?;
    check_field(
        policy,
        &f["entries"],
        &format!("Vec<{}>", r["entry"]),
        true,
        false,
    )?;
    check_field(entry, &f["principal"], "String", false, false)?;
    check_field(entry, &f["restrictions"], &r["restrictions"], false, false)?;
    check_field(restrictions, &f["deny"], "Vec<String>", true, false)?;
    let adapter = check_field(restrictions, &f["allow"], "Option<Vec<String>>", true, true)?
        .ok_or("missing adapter")?;
    check_scope(&file, r, &adapter)?;
    verify_adapter(&file, &adapter)?;
    let declarations: Names = f
        .keys()
        .map(|role| (role.clone(), format!("{}/field/{role}", selection.id)))
        .collect();
    Ok(json!({
        "format":"oddlang-restriction-lists-v1",
        "id":selection.id,
        "sourceSha256":selection.source_sha256,
        "fields":f,
        "declarations":declarations,
        "shape":{
            "sequenceFields":sequence_fields,
            "unknownPolicyFields":"reject", "missingPolicy":"reject", "nullPolicy":"reject",
            "missingVersion":"reject","versionType":"u32",
            "missingDefaults":"emptyRestrictions","missingEntries":"emptyList",
            "missingPrincipal":"reject","missingRestrictions":"reject",
            "missingAllow":"absent","nullAllow":"reject","missingDeny":"emptyList"
        }
    }))
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let graph = graph(&read(source)?, &json(selection)?)?;
    publish(output, &encode(&graph)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = r#"
use serde::{Deserialize, Deserializer};
#[derive(Deserialize)] #[serde(deny_unknown_fields)]
struct Document { edition: u8, palette: Palette }
#[derive(Deserialize)] #[serde(deny_unknown_fields)]
struct Palette { rev: u32, #[serde(default)] base: Swatches, #[serde(default)] overrides: Vec<Theme> }
#[derive(Deserialize)] #[serde(deny_unknown_fields)]
struct Theme { name: String, swatches: Swatches }
#[derive(Default, Deserialize)] #[serde(deny_unknown_fields)]
struct Swatches {
    #[serde(default, deserialize_with = "present")] include: Option<Vec<String>>,
    #[serde(default)] exclude: Vec<String>
}
fn present<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where D: Deserializer<'de>, T: Deserialize<'de>, { T::deserialize(d).map(Some) }
"#;

    fn selection(source: &str) -> Selection {
        serde_json::from_value(json!({
            "format":"oddlang-restriction-lists-selection-v1","id":"urn:test:palette",
            "sourceSha256":digest(source.as_bytes()),
            "records":{"config":"Document","policy":"Palette","entry":"Theme","restrictions":"Swatches"},
            "fields":{"policy":"palette","version":"rev","defaults":"base","entries":"overrides",
                      "principal":"name","restrictions":"swatches","allow":"include","deny":"exclude"}
        })).unwrap()
    }

    #[test]
    fn recovers_unrelated_names_and_checks_the_source_pin() {
        let sel = selection(SOURCE);
        let value = graph(SOURCE.as_bytes(), &sel).unwrap();
        assert_eq!(value["fields"]["allow"], "include");
        assert_eq!(
            value["declarations"]["allow"],
            "urn:test:palette/field/allow"
        );
        assert_eq!(
            value["shape"]["sequenceFields"],
            json!({
                "policy":["version","defaults","entries"],
                "entry":["principal","restrictions"],
                "restrictions":["allow","deny"],
            })
        );
        assert_eq!(value["shape"]["missingAllow"], "absent");
        assert_eq!(value["shape"]["nullAllow"], "reject");
        assert_eq!(value["sourceSha256"], sel.source_sha256);
        assert!(graph(format!("{SOURCE}\n").as_bytes(), &sel).is_err());
    }

    #[test]
    fn rejects_reordered_fields_in_each_positional_record() {
        for (first, second) in [
            ("rev: u32", "#[serde(default)] base: Swatches"),
            ("name: String", "swatches: Swatches"),
            (
                "#[serde(default, deserialize_with = \"present\")] include: Option<Vec<String>>",
                "#[serde(default)] exclude: Vec<String>",
            ),
        ] {
            let reordered = SOURCE
                .replace(first, "__first__")
                .replace(second, first)
                .replace("__first__", second);
            assert!(
                graph(reordered.as_bytes(), &selection(&reordered))
                    .unwrap_err()
                    .contains("positional field order")
            );
        }
    }

    #[test]
    fn rejects_unrecognized_source_semantics_even_when_repinned() {
        for (before, after) in [
            ("palette: Palette", "#[serde(default)] palette: Palette"),
            ("rev: u32", "rev: u64"),
            ("#[serde(default)] base:", "base:"),
            (
                "#[serde(default)] overrides:",
                "#[serde(rename = \"keys\")] overrides:",
            ),
            ("name: String", "name: String, other: bool"),
            ("edition: u8", "#[serde(alias = \"palette\")] edition: u8"),
            ("Option<Vec<String>>", "Option<Vec<bool>>"),
            ("map(Some)", "ok().map(Some)"),
            ("use serde::", "use fake::"),
            ("use serde::", "type String = bool; use serde::"),
            ("use serde::", "use custom::r#Vec; use serde::"),
        ] {
            let altered = SOURCE.replace(before, after);
            assert_ne!(altered, SOURCE);
            assert!(
                graph(altered.as_bytes(), &selection(&altered)).is_err(),
                "{after}"
            );
        }
    }
}
