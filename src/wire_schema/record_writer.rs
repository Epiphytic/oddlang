//! Observe ordered field writes in a restricted, source-pinned native encoder.
use super::{Result, digest, encode, io, iri, json, publish};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use syn::ext::IdentExt;
use syn::{Expr, FnArg, Item, ItemFn, Member, Pat, Stmt, Type, parse_quote};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    source_sha256: String,
    owner: String,
    record: String,
    #[serde(default)]
    module: Vec<String>,
    writer: String,
    #[serde(default)]
    tag_types: BTreeMap<String, String>,
}

fn plain(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(|attr| {
        ["doc", "allow", "warn", "deny", "forbid"]
            .iter()
            .any(|key| attr.path().is_ident(key))
    })
}

fn args(function: &ItemFn) -> Result<Vec<(&syn::Ident, &Type)>> {
    let sig = &function.sig;
    if !plain(&function.attrs)
        || sig.asyncness.is_some()
        || sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || sig.generics != syn::Generics::default()
        || sig.output != syn::ReturnType::Default
    {
        return Err(format!("unsupported writer signature: {}", sig.ident));
    }
    sig.inputs
        .iter()
        .map(|p| {
            if let FnArg::Typed(a) = p
                && a.attrs.is_empty()
                && let Pat::Ident(id) = &*a.pat
                && id.attrs.is_empty()
                && id.by_ref.is_none()
                && id.mutability.is_none()
                && id.subpat.is_none()
            {
                return Ok((&id.ident, &*a.ty));
            }
            Err("writer parameters must be simple bindings".into())
        })
        .collect()
}

fn unique_function<'a>(module: &'a syn::File, name: &str) -> Result<&'a ItemFn> {
    let mut matches = module.items.iter().filter_map(|item| match item {
        Item::Fn(f) if f.sig.ident == name => Some(f),
        _ => None,
    });
    let found = matches
        .next()
        .ok_or_else(|| format!("missing writer function {name}"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous writer function {name}"));
    }
    Ok(found)
}

fn ident(expr: &Expr) -> Option<&syn::Ident> {
    if let Expr::Path(p) = expr
        && p.attrs.is_empty()
        && p.qself.is_none()
    {
        p.path.get_ident()
    } else {
        None
    }
}

fn field<'a>(expr: &'a Expr, record: &syn::Ident) -> Result<&'a syn::Ident> {
    if let Expr::Field(f) = expr
        && f.attrs.is_empty()
        && ident(&f.base) == Some(record)
        && let Member::Named(name) = &f.member
    {
        return Ok(name);
    }
    Err("expected a direct record field".into())
}

fn call_name(expr: &Expr) -> Option<&syn::Ident> {
    if let Expr::Call(call) = expr
        && call.attrs.is_empty()
    {
        ident(&call.func)
    } else {
        None
    }
}

fn unsigned_writer(function: &ItemFn) -> Option<(Type, usize)> {
    let bindings = args(function).ok()?;
    let [(out, output), (n, ty)] = bindings.as_slice() else {
        return None;
    };
    let sink: Type = parse_quote!(&mut Vec<u8>);
    let unsigned: [(Type, usize); 4] = [
        (parse_quote!(u16), 16),
        (parse_quote!(u32), 32),
        (parse_quote!(u64), 64),
        (parse_quote!(u128), 128),
    ];
    let body: syn::Block = parse_quote!({ #out.extend(#n.to_be_bytes()); });
    if *output != &sink || *function.block != body {
        return None;
    }
    unsigned.into_iter().find(|(candidate, _)| candidate == *ty)
}

fn length_writer(module: &syn::File, function: &ItemFn) -> Result<bool> {
    let bindings = args(function)?;
    let [(out, sink), (bytes, params)] = bindings.as_slice() else {
        return Ok(false);
    };
    if **sink != parse_quote!(&mut Vec<u8>) || **params != parse_quote!(&[u8]) {
        return Ok(false);
    }
    let Some(Stmt::Expr(first, _)) = function.block.stmts.first() else {
        return Ok(false);
    };
    let Some(prefix) = call_name(first) else {
        return Ok(false);
    };
    let body: syn::Block = parse_quote!({
        #prefix(#out, #bytes.len() as u32);
        #out.extend(#bytes);
    });
    Ok(*function.block == body
        && unsigned_writer(unique_function(module, &prefix.to_string())?)
            == Some((parse_quote!(u32), 32)))
}

struct Reader<'a> {
    module: &'a syn::File,
    select: &'a Selection,
    types: BTreeMap<String, &'a Type>,
    sink: &'a syn::Ident,
    record: &'a syn::Ident,
    slots: Vec<Value>,
    shapes: BTreeMap<String, Value>,
}

impl Reader<'_> {
    fn ty(&self, field: &syn::Ident) -> Result<&Type> {
        self.types
            .get(&field.unraw().to_string())
            .copied()
            .ok_or_else(|| format!("missing field {field}"))
    }
    fn slot(&mut self, field: &syn::Ident, encoding: &str, shape: Value) {
        let id = format!("{}:field:{}", self.select.owner, field.unraw());
        self.shapes.insert(id.clone(), shape);
        self.slots.push(json!({"field":id,"encoding":encoding}));
    }

    fn write(&mut self, expr: &Expr) -> Result<()> {
        if let Expr::Call(c) = expr
            && let Some(fun) = call_name(expr)
            && c.args.len() == 2
            && ident(&c.args[0]) == Some(self.sink)
        {
            let helper = unique_function(self.module, &fun.to_string())?;
            if let Some((ty, width)) = unsigned_writer(helper) {
                let name = field(&c.args[1], self.record)?;
                if self.ty(name)? == &ty {
                    self.slot(name, "unsigned-be", json!({"kind":"unsigned","bits":width}));
                    return Ok(());
                }
            } else if length_writer(self.module, helper)?
                && let Expr::Reference(r) = &c.args[1]
                && r.attrs.is_empty()
                && r.mutability.is_none()
            {
                let name = field(&r.expr, self.record)?;
                if *self.ty(name)? == parse_quote!(Vec<u8>) {
                    self.slot(
                        name,
                        "u32-be-length-prefixed-bytes",
                        json!({"kind":"bytes","lengthCast":"low-32"}),
                    );
                    return Ok(());
                }
            }
        }
        if let Expr::MethodCall(m) = expr
            && m.attrs.is_empty()
            && m.turbofish.is_none()
            && ident(&m.receiver) == Some(self.sink)
            && m.args.len() == 1
        {
            let arg = &m.args[0];
            if m.method == "extend" {
                if let Expr::Array(a) = arg
                    && a.attrs.is_empty()
                    && !a.elems.is_empty()
                {
                    for item in &a.elems {
                        let Expr::Call(c) = item else {
                            return Err("unsupported array element".into());
                        };
                        if c.args.len() != 1 {
                            return Err("unsupported array element".into());
                        }
                        let name = field(&c.args[0], self.record)?;
                        let record = self.record;
                        if *item != parse_quote!(u8::from(#record.#name))
                            || *self.ty(name)? != parse_quote!(bool)
                        {
                            return Err("expected boolean byte conversion".into());
                        }
                        self.slot(name, "bool-u8", json!({"kind":"bool"}));
                    }
                    return Ok(());
                }
                let name = field(arg, self.record)?;
                if let Type::Array(a) = self.ty(name)?
                    && *a.elem == parse_quote!(u8)
                    && let Expr::Lit(l) = &a.len
                    && let syn::Lit::Int(count) = &l.lit
                {
                    let bytes = count.base10_parse::<usize>().map_err(|e| e.to_string())?;
                    self.slot(
                        name,
                        "fixed-bytes",
                        json!({"kind":"fixed-bytes","bytes":bytes}),
                    );
                    return Ok(());
                }
            }
            if m.method == "push"
                && let Expr::Cast(c) = arg
                && c.attrs.is_empty()
                && *c.ty == parse_quote!(u8)
            {
                let name = field(&c.expr, self.record)?;
                if let Type::Path(p) = self.ty(name)?
                    && p.qself.is_none()
                    && let Some(tag) = p.path.get_ident()
                    && let Some(id) = self.select.tag_types.get(&tag.to_string())
                {
                    self.slot(name, "external-tag-u8", json!({"kind":"external","id":id}));
                    return Ok(());
                }
            }
        }
        Err("write outside the supported ordered record profile".into())
    }
}

fn tag_numbers(
    module: &syn::File,
    sel: &Selection,
) -> Result<BTreeMap<String, BTreeMap<String, u8>>> {
    let mut result = BTreeMap::new();
    for (tag, id) in &sel.tag_types {
        let enums: Vec<_> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Enum(e) if e.ident == tag => Some(e),
                _ => None,
            })
            .collect();
        let [decl] = enums.as_slice() else {
            return Err(format!("expected one local enum for byte tag {tag}"));
        };
        let mut next = Some(0u8);
        let mut codes = BTreeMap::new();
        for variant in &decl.variants {
            if !matches!(variant.fields, syn::Fields::Unit) || !plain(&variant.attrs) {
                return Err(format!("unsupported byte tag variant {}", variant.ident));
            }
            let value = match &variant.discriminant {
                Some((_, Expr::Lit(lit))) => match &lit.lit {
                    syn::Lit::Int(n) => n
                        .base10_parse::<u8>()
                        .map_err(|_| format!("byte tag out of range: {}", variant.ident))?,
                    _ => return Err("non-integer byte tag".into()),
                },
                None => next.ok_or("byte tag overflow")?,
                _ => return Err("unsupported byte tag expression".into()),
            };
            let key = format!("{id}:{}", variant.ident.unraw());
            if codes.values().any(|v| *v == value) || codes.insert(key, value).is_some() {
                return Err("duplicate byte tag".into());
            }
            next = value.checked_add(1);
        }
        if codes.is_empty() || result.insert(id.clone(), codes).is_some() {
            return Err("empty or duplicate byte tag type".into());
        }
    }
    Ok(result)
}

pub fn recover(source: &Path, selection: &Path, out: &Path) -> Result<()> {
    let sel: Selection = json(selection)?;
    iri(&sel.owner)?;
    for id in sel.tag_types.values() {
        iri(id)?;
    }
    let bytes = io(fs::read(source))?;
    let hash = digest(&bytes);
    if hash != sel.source_sha256 {
        return Err("writer source digest mismatch".into());
    }
    let mut module = syn::parse_file(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for name in &sel.module {
        if module.attrs.iter().any(|a| {
            a.path().is_ident("no_implicit_prelude")
                || a.path().is_ident("cfg")
                || a.path().is_ident("cfg_attr")
        }) {
            return Err("unsupported enclosing module configuration".into());
        }
        let mut children = module.items.into_iter().filter_map(|item| match item {
            Item::Mod(m) if m.ident == name => Some(m),
            _ => None,
        });
        let child = children
            .next()
            .ok_or_else(|| format!("missing inline module {name}"))?;
        if children.next().is_some() || !plain(&child.attrs) {
            return Err(format!("ambiguous or decorated inline module {name}"));
        }
        module.attrs = child.attrs;
        module.items = child.content.ok_or("module requires compiler expansion")?.1;
    }
    if !plain(&module.attrs) {
        return Err("conditional or nonstandard module attributes".into());
    }
    let (imports, locals, gaps) = super::recover::lexical_scope(&module);
    for name in ["Vec", "bool", "u8", "u16", "u32", "u64", "u128"] {
        if imports.contains_key(name) || locals.contains(name) || sel.tag_types.contains_key(name) {
            return Err(format!("shadowed or rebound standard type {name}"));
        }
    }
    if let Some(gap) = gaps.first() {
        return Err(format!(
            "unresolved writer scope: {gap}; supply the pinned compiler expansion"
        ));
    }
    for item in &module.items {
        if !super::recover::item_attrs(item).iter().all(|a| {
            plain(std::slice::from_ref(a))
                || [
                    "serde",
                    "repr",
                    "automatically_derived",
                    "must_use",
                    "deprecated",
                    "inline",
                ]
                .iter()
                .any(|name| a.path().is_ident(name))
        }) {
            return Err("unexpanded or decorated item in writer scope; supply the pinned compiler expansion".into());
        }
    }
    let tag_numbers = tag_numbers(&module, &sel)?;
    let records: Vec<_> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) if s.ident == sel.record => Some(s),
            _ => None,
        })
        .collect();
    let [record] = records.as_slice() else {
        return Err("expected one matching record".into());
    };
    if !record.attrs.iter().all(|a| {
        plain(std::slice::from_ref(a)) || a.path().is_ident("serde") || a.path().is_ident("derive")
    }) || record.generics != syn::Generics::default()
    {
        return Err("unsupported record attributes or generics".into());
    }
    let mut fields = BTreeMap::new();
    for f in &record.fields {
        let name = f.ident.as_ref().ok_or("expected named fields")?;
        if !f
            .attrs
            .iter()
            .all(|a| plain(std::slice::from_ref(a)) || a.path().is_ident("serde"))
            || fields.insert(name.unraw().to_string(), &f.ty).is_some()
        {
            return Err(format!("conditional or ambiguous field {name}"));
        }
    }
    let function = unique_function(&module, &sel.writer)?;
    let bindings = args(function)?;
    let [(sink, sink_type), (value, record_type)] = bindings.as_slice() else {
        return Err("writer requires a sink and record".into());
    };
    let name = &record.ident;
    if **sink_type != parse_quote!(&mut Vec<u8>) || **record_type != parse_quote!(&#name) {
        return Err("writer signature does not bind the selected record".into());
    }
    let mut reader = Reader {
        module: &module,
        select: &sel,
        types: fields,
        sink,
        record: value,
        slots: Vec::new(),
        shapes: BTreeMap::new(),
    };
    for (i, statement) in function.block.stmts.iter().enumerate() {
        let Stmt::Expr(expr, Some(_)) = statement else {
            return Err(format!("unsupported writer statement {i}"));
        };
        reader
            .write(expr)
            .map_err(|e| format!("writer statement {i}: {e}"))?;
    }
    if reader.slots.is_empty() {
        return Err("empty writer".into());
    }
    let graph = json!({"format":"oddlang-ordered-field-writes-v1","owner":sel.owner,"fields":reader.slots,"fieldTypes":reader.shapes,"externalTags":tag_numbers});
    let witness = json!({"format":"oddlang-rust-record-writer-observation-v1","sourceSha256":hash,"record":sel.record,"writer":sel.writer,"graphSha256":digest(&encode(&graph)?),"externalTagTypes":sel.tag_types,"module":sel.module,"scope":"lexical","resolutionGaps":gaps});
    publish(
        out,
        &[
            ("graph.json", &encode(&graph)?),
            ("observation.json", &encode(&witness)?),
        ],
    )
}
