//! Recover a public unsigned constant from pinned source and compiler evidence.
use super::*;
use crate::{native_package::publish, wire_schema::iri};
use serde_json::json;
use syn::{BinOp, Expr, Item, Lit, Type, Visibility, spanned::Spanned};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    constant_id: String,
    crate_name: String,
    name: String,
    source_filename: String,
    source_sha256: String,
    rustdoc_sha256: String,
}

#[derive(Deserialize)]
struct Rustdoc {
    format_version: u64,
    root: u64,
    #[serde(deserialize_with = "unique_map")]
    index: BTreeMap<String, DocumentedItem>,
}

#[derive(Deserialize)]
struct DocumentedItem {
    crate_id: u64,
    name: Option<String>,
    span: Option<SourceSpan>,
    visibility: serde_json::Value,
    inner: serde_json::Value,
}

#[derive(Deserialize)]
struct SourceSpan {
    filename: String,
    begin: [usize; 2],
    end: [usize; 2],
}

fn expression(e: &Expr, ty: &str, max: u64) -> Result<(Value, u64)> {
    match e {
        Expr::Lit(e) if e.attrs.is_empty() => {
            let Lit::Int(n) = &e.lit else {
                return Err("expected unsigned integer literal".into());
            };
            if !n.suffix().is_empty() && n.suffix() != ty {
                return Err("literal suffix differs from declared primitive type".into());
            }
            let value: u64 = n
                .base10_parse()
                .map_err(|_| "unsigned literal exceeds u64")?;
            if value > max {
                return Err("unsigned literal exceeds declared width".into());
            }
            Ok((Value::Literal(value), value))
        }
        Expr::Binary(e) if e.attrs.is_empty() && matches!(e.op, BinOp::Mul(_)) => {
            let (left, a) = expression(&e.left, ty, max)?;
            let (right, b) = expression(&e.right, ty, max)?;
            let value = a
                .checked_mul(b)
                .filter(|n| *n <= max)
                .ok_or("unsigned product exceeds declared width")?;
            Ok((
                Value::Product {
                    multiply: [Box::new(left), Box::new(right)],
                },
                value,
            ))
        }
        Expr::Paren(e) if e.attrs.is_empty() => expression(&e.expr, ty, max),
        _ => Err("expected only unsigned literals and multiplication".into()),
    }
}

fn coords(coord: proc_macro2::LineColumn) -> [usize; 2] {
    [coord.line, coord.column + 1]
}

fn recover_bytes(source: &[u8], rustdoc: &[u8], selection: &[u8]) -> Result<[Vec<u8>; 3]> {
    let s: Selection = serde_json::from_slice(selection).map_err(|e| e.to_string())?;
    if s.format != "oddlang-public-constant-selection-v1"
        || digest(source) != s.source_sha256
        || digest(rustdoc) != s.rustdoc_sha256
        || s.source_filename.is_empty()
        || !Path::new(&s.source_filename)
            .components()
            .all(|p| matches!(p, std::path::Component::Normal(_)))
    {
        return Err("invalid public constant selector or evidence pins".into());
    }
    iri(&s.id)?;
    iri(&s.constant_id)?;
    let document: Rustdoc = serde_json::from_slice(rustdoc).map_err(|e| e.to_string())?;
    if document.format_version != 57 {
        return Err("expected rustdoc JSON format 57".into());
    }
    let root = document
        .index
        .get(&document.root.to_string())
        .filter(|root| {
            root.crate_id == 0
                && root.name.as_deref() == Some(s.crate_name.as_str())
                && root.visibility == "public"
                && root.inner["module"]["is_crate"] == true
                && root.inner["module"]["is_stripped"] == false
                && root
                    .span
                    .as_ref()
                    .is_some_and(|span| span.filename == s.source_filename)
        })
        .ok_or("expected the selected local crate root")?;
    let documented: Vec<_> = root.inner["module"]["items"]
        .as_array()
        .ok_or("missing rustdoc crate-root children")?
        .iter()
        .filter_map(|id| document.index.get(&id.as_u64()?.to_string()))
        .filter(|item| item.crate_id == 0 && item.name.as_deref() == Some(&s.name))
        .collect();
    let [documented] = documented.as_slice() else {
        return Err("expected exactly one selected local root child".into());
    };
    if documented.visibility != "public" {
        return Err("rustdoc constant must be public".into());
    }
    let file = syn::parse_file(std::str::from_utf8(source).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let declarations: Vec<_> = file
        .items
        .into_iter()
        .filter_map(|item| match item {
            Item::Const(c) if c.ident == s.name => Some(c),
            _ => None,
        })
        .collect();
    let [c] = declarations.as_slice() else {
        return Err("expected exactly one selected source constant".into());
    };
    let span = documented
        .span
        .as_ref()
        .ok_or("missing constant source span")?;
    if !matches!(c.vis, Visibility::Public(_))
        || !c.attrs.is_empty()
        || span.filename != s.source_filename
        || span.begin != coords(c.span().start())
        || span.end != coords(c.span().end())
    {
        return Err("source and compiler do not identify the same plain public constant".into());
    }
    let Type::Path(ty) = &*c.ty else {
        return Err("expected source primitive integer type".into());
    };
    let name = ty
        .path
        .get_ident()
        .filter(|_| ty.qself.is_none())
        .ok_or("expected unqualified primitive integer type")?
        .to_string();
    let info = &documented.inner["constant"];
    if info["type"] != json!({"primitive": name}) {
        return Err("compiler canonical type differs from source primitive".into());
    }
    let max = match name.as_str() {
        "u8" => u8::MAX as u64,
        "u16" => u16::MAX as u64,
        "u32" => u32::MAX as u64,
        "u64" | "usize" => u64::MAX,
        _ => return Err("unsupported unsigned constant type".into()),
    };
    let (value, actual) = expression(&c.expr, &name, max)?;
    let evaluated = info["const"]["value"]
        .as_str()
        .and_then(|value| value.strip_suffix(&name))
        .and_then(|digits| digits.replace('_', "").parse::<u64>().ok())
        .ok_or("missing compiler-evaluated unsigned constant value")?;
    if actual != evaluated {
        return Err("checked source value disagrees with compiler evaluation".into());
    }
    let graph = json!({"format":"oddlang-shared-declarations-v1","id":s.id,
        "constants":[{"id":s.constant_id,"value":value}]});
    let overlay = json!({"format":"oddlang-shared-rust-v1",
        "names":{(&s.constant_id):s.name},"constantTypes":{(&s.constant_id):name}});
    let g: Graph = serde_json::from_value(graph.clone()).map_err(|e| e.to_string())?;
    let b: Binding = serde_json::from_value(overlay.clone()).map_err(|e| e.to_string())?;
    g.validate(&b)?;
    let encode = |value| {
        serde_json::to_string_pretty(&value)
            .map(|data| (data + "\n").into_bytes())
            .map_err(|e| e.to_string())
    };
    let graph = encode(graph)?;
    let overlay = encode(overlay)?;
    let lock = encode(json!({"format":"oddlang-public-constant-recovery-v1",
        "sourceFilename":s.source_filename,"sourceSha256":s.source_sha256,
        "selectionSha256":digest(selection),"rustdocSha256":s.rustdoc_sha256,
        "rustdocFormatVersion":document.format_version,"unsignedDecimal":actual.to_string(),
        "graphSha256":digest(&graph),"overlaySha256":digest(&overlay)}))?;
    Ok([graph, overlay, lock])
}

pub fn recover(source: &Path, rustdoc: &Path, selection: &Path, output: &Path) -> Result<()> {
    let read = |path: &Path| fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let records = recover_bytes(&read(source)?, &read(rustdoc)?, &read(selection)?)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, bytes) in ["graph.json", "rust.json", "recovery.lock"]
        .into_iter()
        .zip(records)
    {
        fs::write(stage.path().join(name), bytes).map_err(|e| e.to_string())?;
    }
    publish(stage.path(), output)
}
