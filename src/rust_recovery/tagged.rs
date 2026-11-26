//! Source observations for one selected tagged binary variant.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};
use syn::{Arm, Expr, FnArg, ImplItem, Item, Lit, Pat, Stmt, Type, spanned::Spanned};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaggedSelection {
    /// Path relative to the source root.
    pub file: PathBuf,
    /// Inline modules within the selected file. For a module file, use an empty list.
    pub module: Vec<String>,
    pub type_name: String,
    pub variant: String,
    pub encode: String,
    pub decode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaggedCandidate<'a> {
    pub selection: &'a TaggedSelection,
    pub source_sha256: String,
    pub encoded_tag: Observation<u8>,
    pub decoded_tag: Observation<u8>,
    pub fields: Vec<FieldObservation>,
    pub unresolved: &'static [&'static str],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation<T> {
    pub value: T,
    pub line: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldObservation {
    pub name: String,
    pub position: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u8>,
    pub write_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writer_definition_line: Option<usize>,
}

pub fn recover<'a>(root: &Path, selection: &'a TaggedSelection) -> Result<TaggedCandidate<'a>> {
    if selection.file.as_os_str().is_empty()
        || selection
            .file
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("expected a relative Rust source path".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let file = root
        .join(&selection.file)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !file.starts_with(root) {
        return Err("Rust source path is outside the source root".into());
    }
    let source = fs::read_to_string(file).map_err(|error| error.to_string())?;
    let parsed = syn::parse_file(&source).map_err(|error| error.to_string())?;
    let mut items = parsed.items.as_slice();
    unconditional(&parsed.attrs)?;
    for name in &selection.module {
        let module = unique(
            items.iter().filter_map(|item| match item {
                Item::Mod(module) if module.ident == name.as_str() => Some(module),
                _ => None,
            }),
            "inline module",
        )?;
        unconditional(&module.attrs)?;
        items = &module
            .content
            .as_ref()
            .ok_or("expected an inline module")?
            .1;
    }
    let record = unique(
        items.iter().filter_map(|item| match item {
            Item::Enum(record) if record.ident == selection.type_name.as_str() => Some(record),
            _ => None,
        }),
        "enum",
    )?;
    unconditional(&record.attrs)?;
    let variant = unique(
        record
            .variants
            .iter()
            .filter(|variant| variant.ident == selection.variant.as_str()),
        "enum variant",
    )?;
    unconditional(&variant.attrs)?;
    let syn::Fields::Named(declarations) = &variant.fields else {
        return Err("expected a variant with named fields".into());
    };

    let encode = method(items, &record.ident, &selection.encode)?;
    let decode = method(items, &record.ident, &selection.decode)?;
    let (match_expression, payload) = encoder_match(encode)?;
    let encode_arm = encoder_arm(match_expression, record, &variant.ident)?;
    let Pat::Struct(bindings) = &encode_arm.pat else {
        unreachable!()
    };
    let Expr::Block(body) = encode_arm.body.as_ref() else {
        return Err("expected a block in the encoder variant arm".into());
    };
    let (tail, preceding) = body.block.stmts.split_last().ok_or("empty encoder arm")?;
    let Stmt::Expr(tag, None) = tail else {
        return Err("expected the encoder arm to return a literal tag".into());
    };
    let encoded_tag = observe_tag(tag)?;
    let fields = observe_fields(items, declarations, bindings, preceding, payload)?;

    let decoded_tag = decoder_tag(decode, &variant.ident)?;
    if encoded_tag.value != decoded_tag.value {
        return Err("encoder and decoder tags disagree".into());
    }

    Ok(TaggedCandidate {
        selection,
        source_sha256: super::sha256(source.as_bytes()),
        encoded_tag,
        decoded_tag,
        fields,
        unresolved: &[
            "build configuration and compiler name resolution",
            "complete reachable behavior and encoder guard semantics",
            "decoder field consumption, bounds, and error precedence",
            "other variants, stream consumption, and process effects",
        ],
    })
}

fn unique<T>(values: impl IntoIterator<Item = T>, context: &str) -> Result<T> {
    let mut values = values.into_iter();
    let value = values.next().ok_or_else(|| format!("missing {context}"))?;
    if values.next().is_some() {
        return Err(format!("ambiguous {context}"));
    }
    Ok(value)
}

fn unconditional(attributes: &[syn::Attribute]) -> Result<()> {
    if attributes
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
    {
        return Err("conditional syntax needs compiler evidence".into());
    }
    Ok(())
}

fn unconditional_arm(arm: &Arm) -> Result<()> {
    unconditional(&arm.attrs)?;
    if arm.guard.is_some() {
        return Err("guarded variant arm needs control-flow evidence".into());
    }
    Ok(())
}

fn method<'a>(items: &'a [Item], owner: &syn::Ident, name: &str) -> Result<&'a syn::ImplItemFn> {
    let (implementation, method) = unique(
        items
            .iter()
            .filter_map(|item| match item {
                Item::Impl(implementation)
                    if implementation.trait_.is_none()
                        && matches!(implementation.self_ty.as_ref(), Type::Path(path)
                    if path.qself.is_none() && path.path.is_ident(owner)) =>
                {
                    Some(implementation)
                }
                _ => None,
            })
            .flat_map(|implementation| {
                implementation
                    .items
                    .iter()
                    .filter_map(move |item| match item {
                        ImplItem::Fn(method) if method.sig.ident == name => {
                            Some((implementation, method))
                        }
                        _ => None,
                    })
            }),
        "inherent method",
    )?;
    unconditional(&implementation.attrs)?;
    unconditional(&method.attrs)?;
    Ok(method)
}

fn variant_path(path: &syn::Path, variant: &syn::Ident) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == 2
        && path.segments[0].ident == "Self"
        && path.segments[1].ident == *variant
        && path
            .segments
            .iter()
            .all(|segment| segment.arguments.is_none())
}

fn encoder_arm<'a>(
    dispatch: &'a syn::ExprMatch,
    record: &syn::ItemEnum,
    selected: &syn::Ident,
) -> Result<&'a Arm> {
    let (position, selected_arm) = unique(
        dispatch.arms.iter().enumerate().filter(|(_, arm)| {
            matches!(&arm.pat, Pat::Struct(pattern)
                if pattern.qself.is_none() && variant_path(&pattern.path, selected))
        }),
        "encoder variant arm",
    )?;
    unconditional_arm(selected_arm)?;
    for prior in &dispatch.arms[..position] {
        let disjoint = record
            .variants
            .iter()
            .filter(|variant| variant.ident != *selected && unconditional(&variant.attrs).is_ok())
            .any(|variant| match (&prior.pat, &variant.fields) {
                (Pat::Struct(pattern), syn::Fields::Named(_)) => {
                    pattern.qself.is_none() && variant_path(&pattern.path, &variant.ident)
                }
                (Pat::TupleStruct(pattern), syn::Fields::Unnamed(_)) => {
                    pattern.qself.is_none() && variant_path(&pattern.path, &variant.ident)
                }
                (Pat::Path(pattern), syn::Fields::Unit) => {
                    pattern.qself.is_none() && variant_path(&pattern.path, &variant.ident)
                }
                _ => false,
            });
        if !disjoint {
            return Err("preceding encoder arm needs disjointness evidence".into());
        }
    }
    Ok(selected_arm)
}

fn binding(pattern: &Pat) -> Option<&syn::Ident> {
    match pattern {
        Pat::Ident(binding) if binding.by_ref.is_none() && binding.subpat.is_none() => {
            Some(&binding.ident)
        }
        _ => None,
    }
}

fn local(statement: &Stmt) -> Option<(&syn::Ident, &Expr)> {
    let Stmt::Local(local) = statement else {
        return None;
    };
    let init = local.init.as_ref()?;
    if init.diverge.is_some() || !local.attrs.is_empty() {
        return None;
    }
    Some((binding(&local.pat)?, init.expr.as_ref()))
}

fn encoder_match(method: &syn::ImplItemFn) -> Result<(&syn::ExprMatch, &syn::Ident)> {
    let statements = method.block.stmts.as_slice();
    let [
        first,
        second,
        middle @ ..,
        init,
        push,
        length,
        append,
        result,
    ] = statements
    else {
        return Err("unsupported encoder envelope".into());
    };
    let (payload, _) = local(first).ok_or("missing encoder payload")?;
    let (tag, Expr::Match(variants)) = local(second).ok_or("missing encoder dispatch")? else {
        return Err("unsupported encoder dispatch".into());
    };
    let (output, _) = local(init).ok_or("missing encoder output")?;
    if payload == tag
        || payload == output
        || tag == output
        || *variants.expr != syn::parse_quote!(self)
    {
        return Err("unsupported encoder bindings".into());
    }
    let first_expected: Stmt = syn::parse_quote!(let mut #payload = Vec::new(););
    let end: syn::Block = syn::parse_quote!({
        let mut #output = Vec::with_capacity(#payload.len() + 5);
        #output.push(#tag);
        #output.extend((#payload.len() as u32).to_be_bytes());
        #output.extend(#payload);
        Ok(#output)
    });
    if *first != first_expected
        || [init, push, length, append, result]
            .into_iter()
            .ne(&end.stmts)
    {
        return Err("unsupported encoder envelope".into());
    }
    if middle.len() > 1
        || middle
            .first()
            .is_some_and(|statement| !payload_limit_guard(statement, payload))
    {
        return Err("unsupported encoder envelope guard".into());
    }
    Ok((variants, payload))
}

fn early_error(statement: &Stmt) -> Option<&Expr> {
    let Stmt::Expr(Expr::If(condition), _) = statement else {
        return None;
    };
    let [Stmt::Expr(Expr::Return(exit), _)] = &condition.then_branch.stmts[..] else {
        return None;
    };
    let Expr::Call(error) = exit.expr.as_deref()? else {
        return None;
    };
    if condition.else_branch.is_some()
        || !condition.attrs.is_empty()
        || *error.func != syn::parse_quote!(Err)
        || error.args.len() != 1
    {
        return None;
    }
    Some(&condition.cond)
}

fn payload_limit_guard(statement: &Stmt, payload: &syn::Ident) -> bool {
    let Some(Expr::Binary(condition)) = early_error(statement) else {
        return false;
    };
    matches!(condition.op, syn::BinOp::Gt(_))
        && *condition.left == syn::parse_quote!(#payload.len())
        && matches!(condition.right.as_ref(), Expr::Lit(_) | Expr::Path(_))
}

struct Mentions<'a> {
    name: &'a syn::Ident,
    found: bool,
}

impl<'ast> syn::visit::Visit<'ast> for Mentions<'_> {
    fn visit_ident(&mut self, candidate: &'ast syn::Ident) {
        self.found |= self.name == candidate;
    }
}

fn mentions(statement: &Stmt, name: &syn::Ident) -> bool {
    let mut query = Mentions { name, found: false };
    syn::visit::Visit::visit_stmt(&mut query, statement);
    query.found
}

fn observe_tag(tag: &Expr) -> Result<Observation<u8>> {
    let Expr::Lit(tag) = tag else {
        return Err("expected a literal byte tag".into());
    };
    let Lit::Int(number) = &tag.lit else {
        return Err("expected a literal byte tag".into());
    };
    if !tag.attrs.is_empty() || !matches!(number.suffix(), "" | "u8") {
        return Err("expected a literal byte tag".into());
    }
    Ok(Observation {
        value: number
            .base10_parse::<u8>()
            .map_err(|_| "tag is outside the byte range")?,
        line: tag.span().start().line,
    })
}

fn observe_fields(
    items: &[Item],
    declarations: &syn::FieldsNamed,
    bindings: &syn::PatStruct,
    statements: &[Stmt],
    payload: &syn::Ident,
) -> Result<Vec<FieldObservation>> {
    if bindings.rest.is_some() || declarations.named.len() != bindings.fields.len() {
        return Err("expected an encoder binding for every field".into());
    }
    let mut fields = Vec::<FieldObservation>::new();
    for statement in statements {
        // Upstream shape has early-return and fallible validation before writes.
        // These statements are outside the recovered rule subset.
        if fields.is_empty()
            && !mentions(statement, payload)
            && (early_error(statement).is_some()
                || matches!(statement, Stmt::Expr(Expr::Try(value), Some(_))
                if matches!(value.expr.as_ref(), Expr::Call(_))))
        {
            continue;
        }
        let mut recognized = None;
        for binding_field in &bindings.fields {
            unconditional(&binding_field.attrs)?;
            let syn::Member::Named(name) = &binding_field.member else {
                return Err("expected named variant fields".into());
            };
            let variable = binding(&binding_field.pat).ok_or("expected a field binding")?;
            let declaration = unique(
                declarations
                    .named
                    .iter()
                    .filter(|field| field.ident.as_ref() == Some(name)),
                "field declaration",
            )?;
            unconditional(&declaration.attrs)?;
            if let Some((kind, width, writer_definition_line)) =
                observe_write(items, statement, &declaration.ty, variable, payload)?
            {
                recognized = Some(FieldObservation {
                    name: name.to_string(),
                    position: fields.len(),
                    kind,
                    width,
                    write_line: statement.span().start().line,
                    writer_definition_line,
                });
                break;
            }
        }
        let field = recognized.ok_or("unrecognized payload write")?;
        if fields
            .iter()
            .any(|old| old.name == field.name || old.kind == "rest-bytes")
        {
            return Err("repeated field or non-final byte remainder".into());
        }
        fields.push(field);
    }
    if fields.len() != declarations.named.len() {
        return Err("missing payload write for a field".into());
    }
    Ok(fields)
}

fn observe_write(
    items: &[Item],
    statement: &Stmt,
    ty: &Type,
    field: &syn::Ident,
    payload: &syn::Ident,
) -> Result<Option<(&'static str, Option<u8>, Option<usize>)>> {
    let direct_bytes: syn::Block = syn::parse_quote!({ #payload.extend(#field); });
    let byte_type: Type = syn::parse_quote!(Vec<u8>);
    if *ty == byte_type && *statement == direct_bytes.stmts[0] {
        return Ok(Some(("rest-bytes", None, None)));
    }
    let Stmt::Expr(Expr::Call(call), Some(_)) = statement else {
        return Ok(None);
    };
    let Expr::Path(function) = call.func.as_ref() else {
        return Ok(None);
    };
    let Some(name) = function
        .path
        .get_ident()
        .filter(|_| function.qself.is_none())
    else {
        return Ok(None);
    };
    let expected: syn::Block = syn::parse_quote!({ #name(&mut #payload, *#field); });
    if *statement != expected.stmts[0] {
        return Ok(None);
    }
    let Some(width) = [("u8", 1), ("u16", 2), ("u32", 4), ("u64", 8)]
        .into_iter()
        .find_map(|(name, width)| {
            matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident(name))
                .then_some(width)
        })
    else {
        return Ok(None);
    };
    let writer = unique(
        items.iter().filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == *name => Some(function),
            _ => None,
        }),
        "local integer writer",
    )?;
    unconditional(&writer.attrs)?;
    let parameters = writer.sig.inputs.iter().collect::<Vec<_>>();
    let [FnArg::Typed(output), FnArg::Typed(value)] = parameters.as_slice() else {
        return Ok(None);
    };
    let sink: Type = syn::parse_quote!(&mut Vec<u8>);
    if *output.ty != sink || *value.ty != *ty {
        return Ok(None);
    }
    let output = binding(&output.pat).ok_or("expected writer output parameter")?;
    let value = binding(&value.pat).ok_or("expected writer value parameter")?;
    let expected: syn::Block = syn::parse_quote!({ #output.extend(#value.to_be_bytes()); });
    if *writer.block != expected {
        return Ok(None);
    }
    Ok(Some((
        "uint-be",
        Some(width),
        Some(writer.span().start().line),
    )))
}

// The recognized envelope only reads the argument. Extra prefix statements may
// rebind it or supply a different byte slice to the same syntactic match.
fn decoder_input_prefix(statements: &[Stmt], input: &syn::Ident) -> bool {
    let [
        short_header,
        read_length,
        limit,
        length_mismatch,
        read_payload,
    ] = statements
    else {
        return false;
    };
    let Some((length, length_read)) = local(read_length) else {
        return false;
    };
    let Some((payload, payload_read)) = local(read_payload) else {
        return false;
    };
    if input == length || input == payload {
        return false;
    }
    let short: Expr = syn::parse_quote!(#input.len() < 5);
    let length_read_expected: Expr =
        syn::parse_quote!(u32::from_be_bytes(#input[1..5].try_into().unwrap()) as usize);
    let mismatch: Expr = syn::parse_quote!(#input.len() != #length + 5);
    let tail: Expr = syn::parse_quote!(&#input[5..]);
    let reads_tail = match payload_read {
        Expr::Call(call) => {
            matches!(call.func.as_ref(), Expr::Path(path) if path.qself.is_none())
                && call.args.iter().eq([&tail])
        }
        expression => *expression == tail,
    };
    early_error(short_header) == Some(&short)
        && *length_read == length_read_expected
        && matches!(early_error(limit), Some(Expr::Binary(bound))
            if matches!(bound.op, syn::BinOp::Gt(_))
                && *bound.left == syn::parse_quote!(#length)
                && matches!(bound.right.as_ref(), Expr::Lit(_) | Expr::Path(_)))
        && early_error(length_mismatch) == Some(&mismatch)
        && reads_tail
}

fn decoder_tag(method: &syn::ImplItemFn, variant: &syn::Ident) -> Result<Observation<u8>> {
    let input_type: Type = syn::parse_quote!(&[u8]);
    let input = unique(
        method.sig.inputs.iter().filter_map(|input| match input {
            FnArg::Typed(input) if *input.ty == input_type => binding(&input.pat),
            _ => None,
        }),
        "decoder byte slice",
    )?;
    let tag_index: Expr = syn::parse_quote!(#input[0]);
    let (position, binding, dispatch) = unique(
        method
            .block
            .stmts
            .iter()
            .enumerate()
            .filter_map(|(position, stmt)| match local(stmt) {
                Some((binding, Expr::Match(dispatch))) if *dispatch.expr == tag_index => {
                    Some((position, binding, dispatch))
                }
                _ => None,
            }),
        "decoder tag dispatch",
    )?;
    if !decoder_input_prefix(&method.block.stmts[..position], input) {
        return Err("decoder dispatch input lacks provenance".into());
    }
    let (result, finishing) = method.block.stmts[position + 1..]
        .split_last()
        .ok_or("decoder does not return selected dispatch")?;
    let expected: syn::Block = syn::parse_quote!({ Ok(#binding) });
    if *result != expected.stmts[0]
        || finishing
            .iter()
            .any(|statement| mentions(statement, binding))
    {
        return Err("decoder does not return selected dispatch".into());
    }
    let arm = unique(
        dispatch.arms.iter().filter(|arm| {
            let result = match arm.body.as_ref() {
                Expr::Block(body) => match body.block.stmts.last() {
                    Some(Stmt::Expr(expr, None)) => expr,
                    _ => return false,
                },
                result => result,
            };
            matches!(result, Expr::Struct(record) if variant_path(&record.path, variant))
        }),
        "decoder variant arm",
    )?;
    if finishing
        .iter()
        .any(|statement| mentions(statement, variant))
        || dispatch
            .arms
            .iter()
            .filter(|other| !std::ptr::eq(*other, arm))
            .any(|other| {
                let mut query = Mentions {
                    name: variant,
                    found: false,
                };
                syn::visit::Visit::visit_expr(&mut query, &other.body);
                query.found
            })
    {
        return Err("selected decoder variant also occurs outside its dispatch arm".into());
    }
    unconditional_arm(arm)?;
    let Pat::Lit(tag) = &arm.pat else {
        return Err("expected a literal decoder tag".into());
    };
    let tag = observe_tag(&Expr::Lit(tag.clone()))?;
    for prior in dispatch
        .arms
        .iter()
        .take_while(|other| !std::ptr::eq(*other, arm))
    {
        let Pat::Lit(prior_tag) = &prior.pat else {
            return Err("preceding decoder arm needs disjointness evidence".into());
        };
        observe_tag(&Expr::Lit(prior_tag.clone()))?;
    }
    let duplicates = dispatch.arms.iter().any(|other| {
        !std::ptr::eq(other, arm)
            && matches!(&other.pat, Pat::Lit(other_tag)
                if observe_tag(&Expr::Lit(other_tag.clone())).is_ok_and(|other| other.value == tag.value))
    });
    if duplicates {
        return Err("ambiguous decoder tag".into());
    }
    Ok(tag)
}
