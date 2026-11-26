use super::{Consumption, Decimal, FORMAT, Grammar, Notation, Result, Suffix, write};
use crate::wire_schema::{digest, encode};
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeSet, fs, path::Path};
use syn::{
    Expr, ExprMethodCall, FnArg, GenericArgument, Ident, Item, ItemFn, Lit, Pat, PathArguments,
    ReturnType, Stmt, Type, UseTree, ext::IdentExt, punctuated::Punctuated, token::Comma,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    id: String,
    entry: String,
    source_sha256: String,
}

fn same<T: PartialEq>(actual: &T, expected: T) -> bool {
    actual == &expected
}

fn unsupported(part: &str) -> String {
    format!("unsupported text grammar {part}")
}

fn only<T>(values: &Punctuated<T, Comma>) -> Option<&T> {
    (values.len() == 1).then(|| values.first()).flatten()
}

fn binding(pattern: &Pat) -> Result<&Ident> {
    let Pat::Ident(pattern) = pattern else {
        return Err(unsupported("binding"));
    };
    let name = &pattern.ident;
    if reserved(name)
        || !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
    {
        return Err(unsupported("binding"));
    }
    Ok(name)
}

fn parameters(function: &ItemFn) -> Result<Vec<&Ident>> {
    if function
        .attrs
        .iter()
        .any(|attr| !attr.path().is_ident("doc"))
    {
        return Err(unsupported("selected function attributes"));
    }
    let parameters = function
        .sig
        .inputs
        .iter()
        .map(|parameter| match parameter {
            FnArg::Typed(parameter) => binding(&parameter.pat),
            _ => Err(unsupported("parameter")),
        })
        .collect::<Result<Vec<_>>>()?;
    if parameters
        .iter()
        .map(|id| id.unraw())
        .collect::<BTreeSet<_>>()
        .len()
        != parameters.len()
    {
        return Err(unsupported("duplicate parameters"));
    }
    Ok(parameters)
}

fn method(expression: &Expr) -> Result<&ExprMethodCall> {
    let Expr::MethodCall(call) = expression else {
        return Err(unsupported("method call"));
    };
    if call.turbofish.is_some() || !call.attrs.is_empty() {
        return Err(unsupported("method call"));
    }
    Ok(call)
}

fn string(expression: &Expr) -> Result<&syn::LitStr> {
    match expression {
        Expr::Lit(literal) if literal.attrs.is_empty() => match &literal.lit {
            Lit::Str(value) => Ok(value),
            _ => Err(unsupported("literal")),
        },
        _ => Err(unsupported("literal")),
    }
}

fn prefixes(expression: &Expr, input: &Ident, result: &mut Vec<String>) -> Result<()> {
    let call = method(expression)?;
    let argument = only(&call.args).ok_or_else(|| unsupported("prefix argument"))?;
    if call.method == "or_else" {
        prefixes(&call.receiver, input, result)?;
        let Expr::Closure(closure) = argument else {
            return Err(unsupported("literal prefix fallback"));
        };
        let body = &closure.body;
        if !same(argument, syn::parse_quote!(|| #body)) || method(body)?.method != "strip_prefix" {
            return Err(unsupported("literal prefix fallback"));
        }
        return prefixes(body, input, result);
    }
    if call.method != "strip_prefix" || !same(call.receiver.as_ref(), syn::parse_quote!(#input)) {
        return Err(unsupported("literal prefix"));
    }
    result.push(string(argument)?.value());
    Ok(())
}

fn decimal_call(expression: &Expr) -> Result<(&Ident, bool)> {
    let check = method(expression)?;
    if check.method != "is_some" || !check.args.is_empty() {
        return Err(unsupported("decimal option check"));
    }
    let Expr::Call(call) = check.receiver.as_ref() else {
        return Err(unsupported("decimal call"));
    };
    let Expr::Path(path) = call.func.as_ref() else {
        return Err(unsupported("decimal helper reference"));
    };
    let name = path
        .path
        .get_ident()
        .filter(|_| path.qself.is_none())
        .ok_or_else(|| unsupported("decimal helper reference"))?;
    let arguments = call.args.iter().collect::<Vec<_>>();
    let [_, Expr::Lit(positive)] = arguments.as_slice() else {
        return Err(unsupported("decimal arguments"));
    };
    let Lit::Bool(positive) = &positive.lit else {
        return Err(unsupported("decimal minimum flag"));
    };
    Ok((name, positive.value()))
}

struct Entry {
    prefixes: Vec<String>,
    delimiter: String,
    helper: Ident,
    body_positive: bool,
    suffix_positive: bool,
}

fn entry(function: &ItemFn) -> Result<Entry> {
    let arguments = parameters(function)?;
    let [input] = arguments.as_slice() else {
        return Err(unsupported("entry parameters"));
    };
    let name = &function.sig.ident;
    if !same(
        &function.sig,
        syn::parse_quote!(fn #name(#input: &str) -> bool),
    ) {
        return Err(unsupported("entry signature"));
    }
    let [Stmt::Local(prefix), Stmt::Expr(Expr::Match(then), None)] =
        function.block.stmts.as_slice()
    else {
        return Err(unsupported(
            "entry must select a prefix and inspect an optional suffix",
        ));
    };
    let Pat::TupleStruct(remaining) = &prefix.pat else {
        return Err(unsupported("matched prefix binding"));
    };
    let rest =
        binding(only(&remaining.elems).ok_or_else(|| unsupported("matched prefix binding"))?)?;
    let selector = &prefix
        .init
        .as_ref()
        .ok_or_else(|| unsupported("prefix local"))?
        .expr;
    let mut selected = Vec::new();
    prefixes(selector, input, &mut selected)?;

    let split = method(&then.expr)?;
    let delimiter = string(only(&split.args).ok_or_else(|| unsupported("suffix delimiter"))?)?;
    let [some, _] = then.arms.as_slice() else {
        return Err(unsupported("suffix cases"));
    };
    let Pat::TupleStruct(pair) = &some.pat else {
        return Err(unsupported("matched suffix binding"));
    };
    let Some(Pat::Tuple(pair)) = only(&pair.elems) else {
        return Err(unsupported("matched suffix binding"));
    };
    let fields = pair.elems.iter().collect::<Vec<_>>();
    let [body, suffix] = fields.as_slice() else {
        return Err(unsupported("matched suffix binding"));
    };
    let body = binding(body)?;
    let suffix = binding(suffix)?;
    let Expr::Binary(both) = some.body.as_ref() else {
        return Err(unsupported("combined decimal checks"));
    };
    let (helper, body_positive) = decimal_call(&both.left)?;
    let (_, suffix_positive) = decimal_call(&both.right)?;
    if body.unraw() == suffix.unraw()
        || [*input, rest, body, suffix]
            .iter()
            .any(|name| name.unraw() == helper.unraw())
    {
        return Err(unsupported(
            "shadowed decimal call or duplicate slice binding",
        ));
    }
    if !same(
        function.block.as_ref(),
        syn::parse_quote!({
            let Some(#rest) = #selector else { return false; };
            match #rest.split_once(#delimiter) {
                Some((#body, #suffix)) => #helper(#body, #body_positive).is_some()
                                          && #helper(#suffix, #suffix_positive).is_some(),
                None => #helper(#rest, #body_positive).is_some(),
            }
        }),
    ) {
        return Err(unsupported(
            "entry structure or inconsistent decimal checks",
        ));
    }
    Ok(Entry {
        prefixes: selected,
        delimiter: delimiter.value(),
        helper: helper.clone(),
        body_positive,
        suffix_positive,
    })
}

fn decimal_width(function: &ItemFn) -> Result<u8> {
    let args = parameters(function)?;
    let [input, positive] = args.as_slice() else {
        return Err(unsupported("decimal parameters"));
    };
    let ReturnType::Type(_, result) = &function.sig.output else {
        return Err(unsupported("decimal return type"));
    };
    let Type::Path(option) = result.as_ref() else {
        return Err(unsupported("decimal return type"));
    };
    let Some(segment) = option.path.segments.last() else {
        return Err(unsupported("decimal return type"));
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(unsupported("decimal return type"));
    };
    let arguments = arguments.args.iter().collect::<Vec<_>>();
    let [GenericArgument::Type(Type::Path(width))] = arguments.as_slice() else {
        return Err(unsupported("decimal width"));
    };
    let width = width
        .path
        .get_ident()
        .ok_or_else(|| unsupported("decimal width"))?;
    let bits = match width.to_string().as_str() {
        "u8" => 8,
        "u16" => 16,
        "u32" => 32,
        "u64" => 64,
        _ => return Err(unsupported("decimal width")),
    };
    let name = &function.sig.ident;
    if !same(
        &function.sig,
        syn::parse_quote!(fn #name(#input: &str, #positive: bool) -> Option<#width>),
    ) {
        return Err(unsupported("decimal signature"));
    }

    let [
        Stmt::Expr(Expr::If(guard), None),
        Stmt::Local(parsed),
        Stmt::Expr(_, None),
    ] = function.block.stmts.as_slice()
    else {
        return Err(unsupported("decimal validation body"));
    };
    let Expr::Binary(conditions) = guard.cond.as_ref() else {
        return Err(unsupported("decimal digit guard"));
    };
    let Expr::Unary(byte_guard) = conditions.right.as_ref() else {
        return Err(unsupported("decimal digit guard"));
    };
    let all = method(&byte_guard.expr)?;
    let Some(Expr::Closure(closure)) = only(&all.args) else {
        return Err(unsupported("decimal digit guard"));
    };
    let octet = binding(only(&closure.inputs).ok_or_else(|| unsupported("decimal digit guard"))?)?;
    let value = binding(&parsed.pat)?;
    if !same(
        function.block.as_ref(),
        syn::parse_quote!({
            if #input.is_empty()
                || (#input.len() > 1 && #input.starts_with('0'))
                || !#input.bytes().all(|#octet| #octet.is_ascii_digit())
            {
                return None;
            }
            let #value = #input.parse::<#width>().ok()?;
            (!#positive || #value > 0).then_some(#value)
        }),
    ) {
        return Err(unsupported("decimal guards or numeric conversion"));
    }
    Ok(bits)
}

fn reserved(name: &Ident) -> bool {
    matches!(
        name.unraw().to_string().as_str(),
        "Some" | "None" | "Option" | "str" | "bool" | "u8" | "u16" | "u32" | "u64"
    )
}

fn import_shadows(tree: &UseTree, parent: Option<&Ident>) -> bool {
    match tree {
        UseTree::Glob(_) => true,
        UseTree::Path(path) => import_shadows(&path.tree, Some(&path.ident)),
        UseTree::Group(group) => group.items.iter().any(|item| import_shadows(item, parent)),
        UseTree::Rename(rename) => reserved(&rename.rename),
        UseTree::Name(name) if name.ident == "self" => parent.is_some_and(reserved),
        UseTree::Name(name) => reserved(&name.ident),
    }
}

fn check_scope(file: &syn::File) -> Result<()> {
    if file.attrs.iter().any(|attr| !attr.path().is_ident("doc")) {
        return Err(unsupported("source module attributes"));
    }
    for item in &file.items {
        let shadows = match item {
            Item::Use(item) => import_shadows(&item.tree, None),
            Item::Const(item) => reserved(&item.ident),
            Item::Enum(item) => reserved(&item.ident),
            Item::ExternCrate(item) => {
                reserved(item.rename.as_ref().map_or(&item.ident, |(_, id)| id))
            }
            Item::Fn(item) => reserved(&item.sig.ident),
            Item::Macro(_) | Item::Verbatim(_) => true,
            Item::Mod(item) => reserved(&item.ident),
            Item::Static(item) => reserved(&item.ident),
            Item::Struct(item) => reserved(&item.ident),
            Item::Trait(item) => reserved(&item.ident),
            Item::TraitAlias(item) => reserved(&item.ident),
            Item::Type(item) => reserved(&item.ident),
            Item::Union(item) => reserved(&item.ident),
            _ => false,
        };
        if shadows {
            return Err(unsupported(
                "shadowing, glob imports or module-level macro expansions",
            ));
        }
    }
    Ok(())
}

fn function<'a>(file: &'a syn::File, name: &str) -> Result<&'a ItemFn> {
    let matches = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == name => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [function] = matches.as_slice() else {
        return Err(format!(
            "expected one top-level text grammar function named {name}"
        ));
    };
    Ok(function)
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let source = fs::read_to_string(source).map_err(|err| err.to_string())?;
    let selection = fs::read(selection).map_err(|err| err.to_string())?;
    let capture: Selection = serde_json::from_slice(&selection).map_err(|err| err.to_string())?;
    let source_sha256 = digest(source.as_bytes());
    if capture.source_sha256 != source_sha256 {
        return Err("text grammar source digest mismatch".into());
    }
    let file = syn::parse_file(&source).map_err(|err| err.to_string())?;
    check_scope(&file)?;
    let entry = entry(function(&file, &capture.entry)?)?;
    let helper = entry.helper.to_string();
    let bits = decimal_width(function(&file, &helper)?)?;
    let decimal = |positive| Decimal {
        notation: Notation::CanonicalAsciiDecimal,
        bits,
        minimum: u64::from(positive),
    };
    let grammar = Grammar {
        format: FORMAT.into(),
        id: capture.id,
        prefixes: entry.prefixes,
        body: decimal(entry.body_positive),
        suffix: Some(Suffix {
            delimiter: entry.delimiter,
            body: decimal(entry.suffix_positive),
        }),
        consumption: Consumption::EntireInput,
    };
    grammar.validate()?;
    let graph = encode(&grammar)?;
    let lock = encode(&json!({
        "format": "oddlang-text-grammar-recovery-v1",
        "sourceSha256": source_sha256,
        "selectionSha256": digest(&selection),
        "graphSha256": digest(&graph),
        "entry": capture.entry,
        "decimal": helper,
        "unresolved": ["primitive and prelude resolution outside this source module requires a native witness"],
    }))?;
    write(output, &[("graph.json", &graph), ("recovery.lock", &lock)])
}
