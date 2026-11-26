//! Follow explicitly pinned ordinary Rust modules and bound constant names.
use super::*;
use crate::native_package::{self, Pin};
use crate::shared_declarations::unique_map;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use syn::{Expr, Fields, Item, Lit, Member, Meta, ReturnType, Stmt, Type, Visibility, parse_quote};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    #[serde(rename = "crate")]
    main_crate: String,
    module: Vec<String>,
    record: String,
    constructor: String,
    target_env: String,
    revision_env: String,
    revision: String,
    #[serde(deserialize_with = "unique_map")]
    roots: BTreeMap<String, String>,
    sources: Vec<Pin>,
}

struct Sources {
    modules: BTreeMap<String, syn::File>,
    used: BTreeSet<String>,
}

fn plain_attributes(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(|attr| attr.path().is_ident("doc"))
}

fn path(path: &syn::Path) -> Result<Vec<String>> {
    if path.segments.is_empty()
        || path.segments.iter().any(|item| !item.arguments.is_empty())
        || path.leading_colon.is_some()
    {
        return Err("expected ordinary Rust path".into());
    }
    Ok(path
        .segments
        .iter()
        .map(|item| item.ident.to_string())
        .collect())
}

fn integer(ty: &Type) -> Result<Integer> {
    let Type::Path(ty) = ty else {
        return Err("expected primitive integer type".into());
    };
    let name = ty
        .path
        .get_ident()
        .filter(|_| ty.qself.is_none())
        .ok_or("expected unqualified primitive integer type")?
        .to_string();
    let (signed, bits) = if let Some(bits) = name.strip_prefix('i') {
        (true, bits)
    } else if let Some(bits) = name.strip_prefix('u') {
        (false, bits)
    } else {
        return Err("unsupported integer type".into());
    };
    let ty = Integer {
        signed,
        bits: bits.parse().map_err(|_| "unsupported integer type")?,
    };
    if !ty.valid() {
        return Err("unsupported integer type".into());
    }
    Ok(ty)
}

fn literal(expr: &Expr, ty: Integer) -> Result<i128> {
    let (negative, expr) = match expr {
        Expr::Unary(expr) if expr.attrs.is_empty() && matches!(expr.op, syn::UnOp::Neg(_)) => {
            (true, &*expr.expr)
        }
        expr => (false, expr),
    };
    let Expr::Lit(expr) = expr else {
        return Err("expected integer literal".into());
    };
    let Lit::Int(value) = &expr.lit else {
        return Err("expected integer literal".into());
    };
    let suffix = format!("{}{}", if ty.signed { "i" } else { "u" }, ty.bits);
    if !expr.attrs.is_empty() || !matches!(value.suffix(), "") && value.suffix() != suffix {
        return Err("integer literal suffix disagrees with declared width".into());
    }
    let value: i128 = value.base10_parse().map_err(|e| e.to_string())?;
    let value = if negative { -value } else { value };
    if !ty.contains(value) || (negative && !ty.signed) {
        return Err("integer literal outside declared range".into());
    }
    Ok(value)
}

fn import_may_bind(tree: &syn::UseTree, name: &str) -> bool {
    match tree {
        syn::UseTree::Path(path) => path_import_may_bind(&path.tree, name, Some(&path.ident)),
        other => path_import_may_bind(other, name, None),
    }
}

fn path_import_may_bind(tree: &syn::UseTree, name: &str, parent: Option<&syn::Ident>) -> bool {
    match tree {
        syn::UseTree::Path(path) => path_import_may_bind(&path.tree, name, Some(&path.ident)),
        syn::UseTree::Name(value) if value.ident == "self" => {
            parent.is_none_or(|parent| parent == name)
        }
        syn::UseTree::Name(value) => value.ident == name,
        syn::UseTree::Rename(value) => value.rename == name,
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Group(value) => value
            .items
            .iter()
            .any(|tree| path_import_may_bind(tree, name, parent)),
    }
}

fn local_qualifier(file: &syn::File, name: &str) -> bool {
    file.items.iter().any(|item| match item {
        Item::Mod(value) => value.ident == name,
        Item::Use(value) => import_may_bind(&value.tree, name),
        Item::ExternCrate(value) => {
            value
                .rename
                .as_ref()
                .map_or(&value.ident, |(_, rename)| rename)
                == name
        }
        _ => false,
    })
}

impl Sources {
    fn module(
        &mut self,
        root: &str,
        names: &[String],
        externally_visible: bool,
    ) -> Result<&syn::File> {
        let parent = Path::new(root).parent().ok_or("crate root has no parent")?;
        let mut dir = parent.to_path_buf();
        let mut file = root.to_owned();
        for name in names {
            let items = &self.file(&file)?.items;
            let mut declarations = items.iter().filter_map(|item| match item {
                Item::Mod(item) if item.ident == name => Some(item),
                _ => None,
            });
            let module = declarations
                .next()
                .ok_or_else(|| format!("missing module declaration {name}"))?;
            if declarations.next().is_some()
                || (externally_visible && !matches!(module.vis, Visibility::Public(_)))
                || module.content.is_some()
                || !plain_attributes(&module.attrs)
            {
                return Err(format!(
                    "conditional, ambiguous or nonstandard module {name}"
                ));
            }
            let flat = dir.join(format!("{name}.rs"));
            dir.push(name);
            let nested = dir.join("mod.rs");
            let options: Vec<_> = [flat, nested]
                .into_iter()
                .filter_map(|path| {
                    let path = path.to_str()?;
                    self.modules.contains_key(path).then(|| path.to_string())
                })
                .collect();
            let [only] = options.as_slice() else {
                return Err(format!("expected exactly one pinned file for {name}"));
            };
            file = only.clone();
        }
        self.file(&file)
    }
    fn file(&mut self, name: &str) -> Result<&syn::File> {
        self.used.insert(name.to_owned());
        self.modules
            .get(name)
            .ok_or_else(|| format!("missing pinned Rust source {name}"))
    }

    fn constant(
        &mut self,
        selection: &Selection,
        root: &str,
        module: &[String],
        expr: &syn::ExprPath,
        chain: &mut Vec<IntegerDeclaration>,
        external: bool,
    ) -> Result<()> {
        if !expr.attrs.is_empty() || expr.qself.is_some() {
            return Err("unsupported integer declaration reference".into());
        }
        let parts = path(&expr.path)?;
        let (name, scopes) = parts
            .split_last()
            .ok_or("missing integer declaration name")?;
        let (root, modules, crosses_crate) = match scopes.split_first() {
            Some((first, rest)) if first == "crate" => (root, rest.to_vec(), external),
            Some((first, rest)) if first == "self" => (root, [module, rest].concat(), external),
            Some((first, rest)) if selection.roots.contains_key(first) => {
                let current = self.module(
                    selection.roots.get(root).ok_or("unknown crate name")?,
                    module,
                    false,
                )?;
                if local_qualifier(current, first) {
                    return Err(format!("local binding can shadow external crate {first}"));
                }
                (first.as_str(), rest.to_vec(), true)
            }
            _ => (root, [module, scopes].concat(), external),
        };
        // IDs name a declaration, not a particular occurrence in the output array.
        let id = format!(
            "{}:constant:{root}:{}",
            selection.id,
            [modules.as_slice(), &[name.clone()]].concat().join(":")
        );
        if chain.len() >= 64 || chain.iter().any(|decl| decl.id == id) {
            return Err("cyclic or excessively deep constant references".into());
        }
        let file = self.module(
            selection.roots.get(root).ok_or("unknown crate name")?,
            &modules,
            crosses_crate,
        )?;
        let mut matches = file.items.iter().filter_map(|item| match item {
            Item::Const(item) if item.ident == name => Some(item),
            _ => None,
        });
        let item = matches
            .next()
            .ok_or_else(|| format!("missing integer declaration {id}"))?;
        if matches.next().is_some()
            || !plain_attributes(&item.attrs)
            || !item.generics.params.is_empty()
            || item.generics.where_clause.is_some()
            || !constant_visible(&item.vis, module, &modules, crosses_crate)
        {
            return Err(format!(
                "conditional, unsupported, ambiguous or inaccessible integer declaration {id}"
            ));
        }
        let ty = integer(&item.ty)?;
        if chain.last().is_some_and(|prior| prior.integer != ty) {
            return Err("constant alias changes integer type".into());
        }
        if let Expr::Path(target) = item.expr.as_ref() {
            let target = target.clone();
            chain.push(IntegerDeclaration {
                id,
                integer: ty,
                value: None,
            });
            self.constant(selection, root, &modules, &target, chain, false)?;
        } else {
            chain.push(IntegerDeclaration {
                id,
                integer: ty,
                value: Some({
                    let value = literal(&item.expr, ty)?;
                    if ty.signed {
                        Literal::Signed(value as i64)
                    } else {
                        Literal::Unsigned(value as u64)
                    }
                }),
            });
        }
        Ok(())
    }
}

fn constant_visible(
    vis: &Visibility,
    caller: &[String],
    declaration: &[String],
    foreign: bool,
) -> bool {
    match vis {
        Visibility::Public(_) => true,
        Visibility::Inherited => !foreign && caller.starts_with(declaration),
        Visibility::Restricted(scope) => !foreign && scope.path.is_ident("crate"),
    }
}

fn reference(expr: &Expr, dest: Integer) -> Result<(&syn::ExprPath, Conversion)> {
    let (inner, kind) = match expr {
        Expr::Cast(cast) if cast.attrs.is_empty() && integer(&cast.ty)? == dest => {
            (&*cast.expr, Conversion::Wrap)
        }
        Expr::Call(call) if call.attrs.is_empty() && call.args.len() == 1 => {
            let Expr::Path(fun) = &*call.func else {
                return Err("expected unsigned From conversion".into());
            };
            if fun.qself.is_some()
                || !fun.attrs.is_empty()
                || path(&fun.path)? != [format!("u{}", dest.bits), "from".into()]
            {
                return Err("expected unsigned From conversion".into());
            }
            (call.args.first().unwrap(), Conversion::Widen)
        }
        inner => (inner, Conversion::Identity),
    };
    let Expr::Path(inner) = inner else {
        return Err("expected named integer declaration".into());
    };
    Ok((inner, kind))
}

fn string_source(ty: &Type, expr: &Expr, selection: &Selection) -> Result<Source> {
    let expected: Type = parse_quote!(&'static str);
    let Expr::Macro(mac) = expr else {
        return Err("expected compile-time metadata macro".into());
    };
    if *ty != expected || !mac.attrs.is_empty() || !mac.mac.path.is_ident("env") {
        return Err("expected env macro and a static string field".into());
    }
    let name: syn::LitStr = mac.mac.parse_body().map_err(|e| e.to_string())?;
    let name = name.value();
    match name.as_str() {
        "CARGO_PKG_VERSION" => Ok(Source::PackageVersion),
        env if env == selection.target_env => Ok(Source::BuildTarget),
        env if env == selection.revision_env => Ok(Source::Revision {
            value: selection.revision.clone(),
        }),
        _ => Err("unknown compile-time metadata binding".into()),
    }
}

fn wire_name(field: &syn::Field, ident: &syn::Ident) -> Result<String> {
    let attrs: Vec<_> = field
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .collect();
    match attrs.as_slice() {
        [] => Ok(ident.to_string()),
        [attr] if attr.path().is_ident("serde") => {
            let meta = attr.parse_args::<Meta>().map_err(|e| e.to_string())?;
            let Meta::NameValue(meta) = meta else {
                return Err("expected serde rename".into());
            };
            let Expr::Lit(expr) = meta.value else {
                return Err("expected literal wire rename".into());
            };
            let Lit::Str(wire) = expr.lit else {
                return Err("expected literal wire rename".into());
            };
            if !meta.path.is_ident("rename") || !expr.attrs.is_empty() {
                return Err("unsupported serde field attribute".into());
            }
            Ok(wire.value())
        }
        _ => Err("conditional or unsupported field attribute".into()),
    }
}

fn record_fields(
    file: &syn::File,
    selection: &Selection,
) -> Result<Vec<(String, Type, String, Expr)>> {
    let mut found_records = file.items.iter().filter_map(|item| match item {
        Item::Struct(item) if item.ident == selection.record => Some(item),
        _ => None,
    });
    let record = found_records.next().ok_or("missing report record")?;
    let mut found_functions = file.items.iter().filter_map(|item| match item {
        Item::Fn(item) if item.sig.ident == selection.constructor => Some(item),
        _ => None,
    });
    let function = found_functions.next().ok_or("missing record constructor")?;
    let ser: syn::Attribute = parse_quote!(#[derive(Serialize)]);
    let qualified: syn::Attribute = parse_quote!(#[derive(serde::Serialize)]);
    let derive: Vec<_> = record
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .collect();
    let serde_import: syn::ItemUse = parse_quote!(
        use serde::Serialize;
    );
    let derive_matches = derive.len() == 1
        && (*derive[0] == qualified
            || (*derive[0] == ser
                && file
                    .items
                    .iter()
                    .any(|item| matches!(item, Item::Use(u) if *u == serde_import))));
    if !derive_matches
        || found_records.next().is_some()
        || found_functions.next().is_some()
        || !record.generics.params.is_empty()
        || record.generics.where_clause.is_some()
        || !plain_attributes(&function.attrs)
        || !function.sig.inputs.is_empty()
        || !function.sig.generics.params.is_empty()
        || function.sig.generics.where_clause.is_some()
        || function.sig.constness.is_some()
        || function.sig.asyncness.is_some()
        || function.sig.unsafety.is_some()
        || function.sig.abi.is_some()
        || function.sig.variadic.is_some()
    {
        return Err(
            "conditional, ambiguous or unsupported serialized record or constructor".into(),
        );
    }
    let name = &record.ident;
    let expected: ReturnType = parse_quote!(-> #name);
    let Fields::Named(fields) = &record.fields else {
        return Err("expected named record fields".into());
    };
    let [Stmt::Expr(Expr::Struct(expr), None)] = function.block.stmts.as_slice() else {
        return Err("constructor must return one record without effects".into());
    };
    if function.sig.output != expected
        || !expr.path.is_ident(name)
        || !expr.attrs.is_empty()
        || expr.qself.is_some()
        || expr.rest.is_some()
    {
        return Err("constructor does not return the selected record".into());
    }
    let mut values = BTreeMap::new();
    for value in &expr.fields {
        let Member::Named(name) = &value.member else {
            return Err("expected named initializer".into());
        };
        if !value.attrs.is_empty() || values.insert(name, &value.expr).is_some() {
            return Err("conditional or duplicate field initializer".into());
        }
    }
    if fields.named.len() != values.len() {
        return Err("constructor must initialize every field".into());
    }
    let mut output = Vec::new();
    for field in &fields.named {
        let name = field.ident.as_ref().ok_or("missing field identifier")?;
        let value = values.remove(name).ok_or("missing field initializer")?;
        output.push((
            name.to_string(),
            field.ty.clone(),
            wire_name(field, name)?,
            value.clone(),
        ));
    }
    Ok(output)
}

fn recover_field(
    sources: &mut Sources,
    selection: &Selection,
    ty: &Type,
    expr: &Expr,
) -> Result<(Source, Value)> {
    match ty {
        Type::Array(array) => {
            let bits = integer(&array.elem)?;
            if bits.signed {
                return Err("report arrays require unsigned elements".into());
            }
            let length = usize::try_from(literal(
                &array.len,
                Integer {
                    signed: false,
                    bits: 64,
                },
            )?)
            .map_err(|_| "invalid report array length")?;
            let Expr::Array(values) = expr else {
                return Err("expected ordered array initializer".into());
            };
            if !values.attrs.is_empty() || values.elems.len() != length {
                return Err("array initializer length or conditional mismatch".into());
            }
            let mut items = Vec::new();
            for value in &values.elems {
                let (expr, conversion) = reference(value, bits)?;
                let mut chain = Vec::new();
                sources.constant(
                    selection,
                    &selection.main_crate,
                    &selection.module,
                    expr,
                    &mut chain,
                    false,
                )?;
                items.push(Reference { chain, conversion });
            }
            let value = resolved(bits.bits, length, &items)?;
            Ok((
                Source::UnsignedReferenceList {
                    bits: bits.bits,
                    length,
                    items,
                },
                json!(value),
            ))
        }
        Type::Reference(_) => {
            let source = string_source(ty, expr, selection)?;
            let value = match &source {
                Source::Revision { value } => json!(value),
                _ => Value::Null,
            };
            Ok((source, value))
        }
        _ => {
            let ty = integer(ty)?;
            if ty.signed {
                return Err("report fields require unsigned integers".into());
            }
            let value = literal(expr, ty)? as u64;
            Ok((
                Source::Unsigned {
                    bits: ty.bits,
                    value,
                },
                json!(value),
            ))
        }
    }
}

pub fn recover(base: &Path, selection: &Path, out: &Path) -> Result<()> {
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    let raw = fs::read(selection).map_err(|e| e.to_string())?;
    let selection: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if selection.format != "oddlang-build-report-recovery-v1"
        || selection.target_env == selection.revision_env
        || [&selection.target_env, &selection.revision_env]
            .iter()
            .any(|name| name.as_str() == "CARGO_PKG_VERSION")
        || !selection.module.iter().all(|x| symbol(x))
        || !symbol(&selection.main_crate)
        || !symbol(&selection.record)
        || !symbol(&selection.constructor)
        || !selection.roots.keys().all(|x| symbol(x))
    {
        return Err("invalid build-report source selection".into());
    }
    let mut sources = Sources {
        modules: BTreeMap::new(),
        used: BTreeSet::new(),
    };
    let mut pins = BTreeMap::new();
    for pin in &selection.sources {
        let bytes = pin.read(&base)?;
        let source = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let module = syn::parse_file(source).map_err(|e| e.to_string())?;
        if !plain_attributes(&module.attrs) {
            return Err(format!(
                "conditional or unsupported module-level attributes: {}",
                pin.path
            ));
        }
        if sources.modules.insert(pin.path.clone(), module).is_some() {
            return Err("duplicate source pin".into());
        }
        pins.insert(&pin.path, digest(&bytes));
    }
    let root = selection
        .roots
        .get(&selection.main_crate)
        .ok_or("missing primary crate root")?;
    let declarations = record_fields(sources.module(root, &selection.module, false)?, &selection)?;
    let mut report = Report {
        format: "oddlang-build-report-v1".into(),
        id: selection.id.clone(),
        fields: vec![],
        guards: vec![],
    };
    let mut values = Vec::new();
    for (name, ty, wire, expr) in declarations {
        let (source, value) = recover_field(&mut sources, &selection, &ty, &expr)?;
        report.fields.push(Field {
            id: format!("{}:{name}", selection.id),
            wire,
            source,
        });
        values.push(value);
    }
    if !sources.used.iter().eq(sources.modules.keys()) {
        return Err("unused source pins".into());
    }
    let graph = encode(&report)?;
    // Use the same graph admission as the package compiler. The recovery has no CLI binding.
    load(&graph, br#"{"entry":"recovered_report"}"#)?;
    let receipt = encode(&json!({
        "format": "oddlang-build-report-source-observation-v1",
        "selectionSha256": digest(&raw), "graphSha256": digest(&graph), "sources": pins,
        "values": values,
        "gaps": ["serde, external crate names and build environment role bindings require native replay",
                 "CLI guard, invocation and output effects are selected separately"],
    }))?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if out.symlink_metadata().is_ok() {
        return Err("build report recovery output already exists".into());
    }
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("graph.json"), graph).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("observation.json"), receipt).map_err(|e| e.to_string())?;
    native_package::publish(stage.path(), out)
}
