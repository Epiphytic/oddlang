//! Recover declarative constants, display errors and result aliases from a pinned Rust module.
use oxigraph::model::NamedNode;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
    process::Command,
};
use syn::{
    Attribute, Expr, Fields, GenericArgument, GenericParam, Item, Lit, PathArguments, Type,
    Visibility,
};

type Result<T> = std::result::Result<T, String>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    source_revision: String,
    module_file: String,
    domain: String,
    constants: BTreeMap<String, String>,
    errors: BTreeMap<String, SelectedEnum>,
    results: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedEnum {
    id: String,
    variants: BTreeMap<String, String>,
}

fn attributes(attrs: &[Attribute], allowed: &[&str]) -> Result<()> {
    for attribute in attrs {
        if !attribute.path().is_ident("doc")
            && !allowed.iter().any(|key| attribute.path().is_ident(key))
        {
            return Err("unsupported attribute on selected declaration".into());
        }
    }
    Ok(())
}

fn native_error_traits(attrs: &[Attribute]) -> Result<()> {
    use syn::{Token, punctuated::Punctuated};
    let expected: BTreeSet<_> = [
        "Debug",
        "Copy",
        "Clone",
        "Eq",
        "PartialEq",
        "thiserror::Error",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut derived = BTreeSet::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("derive")) {
        let paths = attr
            .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
            .map_err(|e| e.to_string())?;
        for path in paths {
            let name = path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if !derived.insert(name) {
                return Err("duplicate error derive".into());
            }
        }
    }
    if derived != expected {
        return Err("error derive set differs from the supported native binding".into());
    }
    Ok(())
}
fn simple(ty: &Type) -> Result<String> {
    if let Type::Path(p) = ty
        && p.qself.is_none()
        && let Some(i) = p.path.get_ident()
    {
        return Ok(i.to_string());
    }
    Err("expected a simple named type".into())
}

fn number(e: &Expr, constants: &BTreeMap<String, String>) -> Result<Value> {
    match e {
        Expr::Lit(l) if let Lit::Int(i) = &l.lit => {
            Ok(json!(i.base10_parse::<u64>().map_err(|e| e.to_string())?))
        }
        Expr::Paren(p) => number(&p.expr, constants),
        Expr::Path(p) if let Some(key) = p.path.get_ident() => Ok(
            json!({"ref":constants.get(&key.to_string()).ok_or("constant references an unselected name")?}),
        ),
        Expr::Binary(b) if matches!(b.op, syn::BinOp::Mul(_)) => {
            Ok(json!({"multiply":[number(&b.left,constants)?,number(&b.right,constants)?]}))
        }
        Expr::Cast(c)
            if simple(&c.ty)? == "u64"
                && let Expr::Path(p) = &*c.expr
                && p.qself.is_none() =>
        {
            let path: Vec<_> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            match path
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["i64", "MAX"] => Ok(json!(i64::MAX as u64)),
                ["u32", "MAX"] => Ok(json!(u32::MAX)),
                _ => Err("unsupported typed constant boundary".into()),
            }
        }
        _ => Err("unsupported numeric constant expression".into()),
    }
}

fn payload(fields: &Fields) -> Result<&'static str> {
    match fields {
        Fields::Unit => Ok("none"),
        Fields::Unnamed(f) if f.unnamed.len() == 1 => {
            let field = &f.unnamed[0];
            attributes(&field.attrs, &[])?;
            match &field.ty {
                Type::Reference(r)
                    if r.mutability.is_none()
                        && r.lifetime.as_ref().is_some_and(|t| t.ident == "static")
                        && simple(&r.elem)? == "str" =>
                {
                    Ok("text")
                }
                t => match simple(t)?.as_str() {
                    "u64" => Ok("u64"),
                    "bool" => Ok("bool"),
                    _ => Err("unsupported error payload".into()),
                },
            }
        }
        _ => Err("expected a unit error or a single unnamed payload".into()),
    }
}

fn message(template: &str, kind: &str) -> Result<Vec<Value>> {
    let mut chars = template.chars().peekable();
    let mut text = String::new();
    let mut pieces = vec![];
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('{', Some('{')) | ('}', Some('}')) => {
                chars.next();
                text.push(c);
            }
            ('{', Some('0')) if kind != "none" => {
                chars.next();
                if chars.next() != Some('}') {
                    return Err("unsupported display placeholder".into());
                }
                if !text.is_empty() {
                    pieces.push(json!({"text":std::mem::take(&mut text)}));
                }
                pieces.push(json!({"payload":true}));
            }
            ('{' | '}', _) => return Err("unsupported display brace".into()),
            _ => text.push(c),
        }
    }
    if !text.is_empty() || pieces.is_empty() {
        pieces.push(json!({"text":text}));
    }
    Ok(pieces)
}

fn alias(i: &syn::ItemType) -> Result<String> {
    let params: Vec<_> = i.generics.params.iter().collect();
    let [GenericParam::Type(param)] = params.as_slice() else {
        return Err("expected a single result type parameter".into());
    };
    if !param.bounds.is_empty() || param.default.is_some() || i.generics.where_clause.is_some() {
        return Err("unsupported result alias bounds".into());
    }
    let Type::Path(path) = i.ty.as_ref() else {
        return Err("expected native Result".into());
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if !(segments == ["std", "result", "Result"] || segments == ["core", "result", "Result"]) {
        return Err("expected std/core Result".into());
    }
    let PathArguments::AngleBracketed(a) = &path.path.segments.last().unwrap().arguments else {
        return Err("missing Result arguments".into());
    };
    let args = a.args.iter().collect::<Vec<_>>();
    match args.as_slice() {
        [GenericArgument::Type(t), GenericArgument::Type(err)]
            if simple(t)? == param.ident.to_string() =>
        {
            simple(err)
        }
        _ => Err("unsupported Result arguments".into()),
    }
}

fn extract(s: &Selection, src: &str) -> Result<(Value, Value)> {
    let file = syn::parse_file(src).map_err(|e| e.to_string())?;
    attributes(&file.attrs, &[])?;
    let mut constants = vec![];
    let mut errors = vec![];
    let mut results = vec![];
    let mut names = BTreeMap::new();
    let mut types = BTreeMap::new();
    let mut orders = BTreeMap::new();
    let mut reserved = BTreeSet::new();
    for id in std::iter::once(&s.domain)
        .chain(s.constants.values())
        .chain(s.results.values())
        .chain(
            s.errors
                .values()
                .flat_map(|e| std::iter::once(&e.id).chain(e.variants.values())),
        )
    {
        NamedNode::new(id.as_str()).map_err(|e| e.to_string())?;
        if !reserved.insert(id) {
            return Err(format!("duplicate selected identity {id}"));
        }
    }
    for item in file.items {
        match item {
            Item::Const(i) if let Some(id) = s.constants.get(&i.ident.to_string()) => {
                attributes(&i.attrs, &[])?;
                public(&i.vis)?;
                let ty = simple(&i.ty)?;
                if !matches!(ty.as_str(), "usize" | "u64" | "u32" | "u16" | "u8") {
                    return Err("unsupported constant type".into());
                }
                constants.push(json!({"id":id,"value":number(&i.expr,&s.constants)?}));
                types.insert(id, ty);
                names.insert(id, i.ident.to_string());
            }
            Item::Enum(i) if let Some(e) = s.errors.get(&i.ident.to_string()) => {
                attributes(&i.attrs, &["derive"])?;
                native_error_traits(&i.attrs)?;
                public(&i.vis)?;
                if !i.generics.params.is_empty() || i.generics.where_clause.is_some() {
                    return Err("generic errors unsupported".into());
                }
                let mut variants = vec![];
                let mut order = vec![];
                for v in i.variants {
                    attributes(&v.attrs, &["error"])?;
                    if v.discriminant.is_some() {
                        return Err("explicit ordinal outside this binding".into());
                    }
                    let id = e
                        .variants
                        .get(&v.ident.to_string())
                        .ok_or("missing error variant identity")?;
                    let kind = payload(&v.fields)?;
                    let attrs = v
                        .attrs
                        .iter()
                        .filter(|a| a.path().is_ident("error"))
                        .collect::<Vec<_>>();
                    let [attr] = attrs.as_slice() else {
                        return Err("expected one literal display annotation".into());
                    };
                    let template = attr
                        .parse_args::<syn::LitStr>()
                        .map_err(|e| e.to_string())?
                        .value();
                    variants
                        .push(json!({"id":id,"payload":kind,"message":message(&template,kind)?}));
                    order.push(id);
                    names.insert(id, v.ident.to_string());
                }
                if order.len() != e.variants.len() {
                    return Err("extraneous error variant selection".into());
                }
                orders.insert(&e.id, order);
                names.insert(&e.id, i.ident.to_string());
                errors.push(json!({"id":e.id,"variants":variants}));
            }
            Item::Type(i) if let Some(id) = s.results.get(&i.ident.to_string()) => {
                attributes(&i.attrs, &[])?;
                public(&i.vis)?;
                let name = alias(&i)?;
                results.push(json!({"id":id,"error":s.errors.get(&name).ok_or("unselected result error")?.id}));
                names.insert(id, i.ident.to_string());
            }
            _ => (),
        }
    }
    if names.len() + 1 != reserved.len()
        || constants.len() != s.constants.len()
        || errors.len() != s.errors.len()
        || results.len() != s.results.len()
    {
        return Err("selected declarations are missing or repeated".into());
    }
    Ok((
        json!({"format":"oddlang-shared-declarations-v1","id":s.domain,"constants":constants,"errors":errors,"results":results}),
        json!({"format":"oddlang-shared-rust-v1","names":names,"variantOrder":orders,"constantTypes":types}),
    ))
}
fn public(v: &Visibility) -> Result<()> {
    if matches!(v, Visibility::Public(_)) {
        Ok(())
    } else {
        Err("selected declaration is not public".into())
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn run(repo: &Path, selection: &Path, out: &Path) -> Result<()> {
    let raw = fs::read(selection).map_err(|e| e.to_string())?;
    let s: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if s.source_revision.len() != 40
        || !s.source_revision.bytes().all(|b| b.is_ascii_hexdigit())
        || !Path::new(&s.module_file)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("expected full Git commit and relative module path".into());
    }
    let git = |args: &[&str]| -> Result<Vec<u8>> {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .arg("--no-replace-objects")
            .args(args)
            .current_dir(repo)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(output.stdout)
    };
    if git(&["cat-file", "-t", &s.source_revision])? != b"commit\n" {
        return Err("pin must name a commit".into());
    }
    let blob = git(&[
        "cat-file",
        "blob",
        &format!("{}:{}", s.source_revision, s.module_file),
    ])?;
    let (graph, binding) = extract(&s, std::str::from_utf8(&blob).map_err(|e| e.to_string())?)?;
    let receipt = json!({"qualification":"source-declaration-candidate","sourceRevision":s.source_revision,
        "moduleFile":s.module_file,"moduleSha256":digest(&blob),"selectionSha256":digest(&raw)});
    let prospective = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::tempdir_in(prospective).map_err(|e| e.to_string())?;
    for (file, data) in [
        ("graph.json", graph),
        ("rust.json", binding),
        ("observation.json", receipt),
    ] {
        fs::write(
            staging.path().join(file),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?
            ),
        )
        .map_err(|e| e.to_string())?;
    }
    if out.exists() {
        return Err("output already exists".into());
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, staging.path(), CWD, out, RenameFlags::NOREPLACE)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        fs::rename(staging.path(), out).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = r#"
pub const LIMIT:u64=i64::MAX as u64;
pub const OTHER:u64=u32::MAX as u64;
#[derive(Debug,Clone,Copy,PartialEq,Eq,thiserror::Error)]
pub enum First { #[error("bad {0}")] Text(&'static str), #[error("flag {0}")] Flag(bool) }
#[derive(Debug,Clone,Copy,PartialEq,Eq,thiserror::Error)]
pub enum Second { #[error("missing")] Missing }
pub type Output<T>=std::result::Result<T,First>;
"#;
    fn selection() -> Selection {
        serde_json::from_value(
            json!({"sourceRevision":"","moduleFile":"lib.rs","domain":"urn:x:d",
           "constants":{"LIMIT":"urn:x:l","OTHER":"urn:x:o"},"results":{"Output":"urn:x:r"},
           "errors":{"First":{"id":"urn:x:e","variants":{"Text":"urn:x:t","Flag":"urn:x:f"}},
                     "Second":{"id":"urn:x:s","variants":{"Missing":"urn:x:m"}}}}),
        )
        .unwrap()
    }
    #[test]
    fn selected_native_structure_tracks_the_source() {
        let s = selection();
        let (g, r) = extract(&s, SOURCE).unwrap();
        assert_eq!(g["constants"][0]["value"], json!(i64::MAX));
        assert_eq!(g["constants"][1]["value"], json!(u32::MAX));
        assert_eq!(g["errors"][0]["variants"][0]["payload"], "text");
        assert_eq!(g["errors"][0]["variants"][1]["payload"], "bool");
        let edited = SOURCE.replace("T,First", "T,Second").replace(
            r#"#[error("bad {0}")] Text(&'static str), #[error("flag {0}")] Flag(bool)"#,
            r#"#[error("flag {0}")] Flag(bool), #[error("bad {0}")] Text(&'static str)"#,
        );
        let (new_g, new_r) = extract(&s, &edited).unwrap();
        assert_eq!(new_g["results"][0]["error"], "urn:x:s");
        assert_eq!(r["variantOrder"]["urn:x:e"], json!(["urn:x:t", "urn:x:f"]));
        assert_eq!(
            new_r["variantOrder"]["urn:x:e"],
            json!(["urn:x:f", "urn:x:t"])
        );
        assert_eq!(new_r["constantTypes"]["urn:x:l"], "u64");
        let mut missing = selection();
        missing
            .errors
            .get_mut("First")
            .unwrap()
            .variants
            .remove("Flag");
        assert!(extract(&missing, SOURCE).is_err());
        let mut extra = selection();
        extra
            .errors
            .get_mut("First")
            .unwrap()
            .variants
            .insert("Extra".into(), "urn:x:extra".into());
        assert!(extract(&extra, SOURCE).is_err());
        let mut duplicate = selection();
        duplicate.constants.insert("OTHER".into(), "urn:x:l".into());
        assert!(extract(&duplicate, SOURCE).is_err());
    }
}
