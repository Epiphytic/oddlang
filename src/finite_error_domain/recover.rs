//! Recover a constrained subset of ordinary thiserror enums and exhaustive literal tables.
use super::*;
use syn::{
    Attribute, Expr, Fields, Item, ItemFn, Lit, LitStr, Pat, ReturnType, Type, UseTree, Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    token::Comma,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    source_sha256: String,
    error: String,
    result: String,
}

struct Diagnostic {
    text: String,
    helper: Option<String>,
}

impl Parse for Diagnostic {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let text = input.parse::<LitStr>()?.value();
        let helper = if input.is_empty() {
            None
        } else {
            input.parse::<syn::Token![,]>()?;
            let name = input.parse::<syn::Ident>()?.to_string();
            let arg;
            syn::parenthesized!(arg in input);
            arg.parse::<syn::Token![.]>()?;
            let index = arg.parse::<syn::LitInt>()?;
            if index.to_string() != "0" || !arg.is_empty() {
                return Err(arg.error("requires category lookup of the sole payload"));
            }
            Some(name)
        };
        Ok(Self { text, helper })
    }
}

fn docs(attrs: &[Attribute]) -> bool {
    attrs.iter().all(|attr| attr.path().is_ident("doc"))
}

fn strict_path(path: &syn::Path) -> Result<Vec<String>> {
    if path
        .segments
        .iter()
        .any(|segment| !segment.arguments.is_empty())
    {
        return Err("unsupported generic path in error recovery".into());
    }
    Ok(path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect())
}

fn payload_path(ty: &Type) -> Result<Vec<String>> {
    let Type::Path(path) = ty else {
        return Err("expected plain category type path".into());
    };
    if path.qself.is_some() {
        return Err("qualified category path is unsupported".into());
    }
    strict_path(&path.path)
}

fn visibility_pub_crate(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Restricted(value)
        if value.in_token.is_none() && value.path.is_ident("crate"))
}

fn literal_table(helper: &ItemFn, ty: &Type) -> Result<BTreeMap<String, String>> {
    let signature = &helper.sig;
    let output: ReturnType = parse_quote!(-> &'static str);
    if !visibility_pub_crate(&helper.vis)
        || !docs(&helper.attrs)
        || signature.constness.is_some()
        || signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
        || !signature.generics.params.is_empty()
        || signature.generics.where_clause.is_some()
        || signature.output != output
    {
        return Err("unsupported category helper signature".into());
    }
    let Some(syn::FnArg::Typed(input)) = signature.inputs.first() else {
        return Err("category helper needs one borrowed argument".into());
    };
    let expected_input: Type = parse_quote!(&#ty);
    let Pat::Ident(argument) = input.pat.as_ref() else {
        return Err("invalid category helper binding".into());
    };
    if signature.inputs.len() != 1
        || *input.ty != expected_input
        || !input.attrs.is_empty()
        || argument.by_ref.is_some()
        || argument.mutability.is_some()
        || argument.subpat.is_some()
        || !argument.attrs.is_empty()
    {
        return Err("unsupported category argument binding".into());
    }
    let [
        syn::Stmt::Item(Item::Use(import)),
        syn::Stmt::Expr(Expr::Match(branches), None),
    ] = helper.block.stmts.as_slice()
    else {
        return Err("category helper needs an enum glob and a final match".into());
    };
    if !import.attrs.is_empty()
        || !matches!(import.vis, Visibility::Inherited)
        || !branches.attrs.is_empty()
        || *branches.expr != parse_quote!(#argument)
    {
        return Err("category helper must match its borrowed argument".into());
    }
    let mut imported = Vec::new();
    let mut tree = &import.tree;
    while let UseTree::Path(part) = tree {
        imported.push(part.ident.to_string());
        tree = &part.tree;
    }
    if !matches!(tree, UseTree::Glob(_)) || imported != payload_path(ty)? {
        return Err("category helper must import the matched enum variants".into());
    }
    let mut result = BTreeMap::new();
    for arm in &branches.arms {
        let (Pat::Ident(member), Expr::Lit(label)) = (&arm.pat, &*arm.body) else {
            return Err("category table requires unit variants and string labels".into());
        };
        let Lit::Str(label_value) = &label.lit else {
            return Err("category table requires string labels".into());
        };
        if !arm.attrs.is_empty()
            || arm.guard.is_some()
            || !member.attrs.is_empty()
            || member.by_ref.is_some()
            || member.mutability.is_some()
            || member.subpat.is_some()
            || !label.attrs.is_empty()
            || result
                .insert(member.ident.to_string(), label_value.value())
                .is_some()
        {
            return Err("unsupported or repeated category match arm".into());
        }
    }
    Ok(result)
}

fn check_alias(alias: &syn::ItemType, error: &syn::Ident) -> Result<()> {
    let expected: Type = parse_quote!(std::result::Result<T, #error>);
    let expected_generics: syn::Generics = parse_quote!(<T>);
    if !matches!(alias.vis, Visibility::Public(_))
        || !docs(&alias.attrs)
        || alias.generics != expected_generics
        || *alias.ty != expected
    {
        return Err("expected ordinary public single-type-parameter Result alias".into());
    }
    Ok(())
}

fn message(diagnostic: &Diagnostic, field: bool) -> Result<Message> {
    let marker = if diagnostic.helper.is_some() {
        "{}"
    } else {
        "{0}"
    };
    let braces = |text: &str| text.contains(['{', '}']);
    if field
        && let Some((prefix, suffix)) = diagnostic.text.split_once(marker)
        && !braces(prefix)
        && !braces(suffix)
    {
        return Ok(Message::Insertion {
            prefix: prefix.into(),
            suffix: suffix.into(),
        });
    }
    if diagnostic.helper.is_none() && !braces(&diagnostic.text) {
        return Ok(Message::Literal {
            text: diagnostic.text.clone(),
        });
    }
    Err("unsupported diagnostic interpolation".into())
}

fn derive_error(error: &syn::ItemEnum) -> Result<()> {
    let attrs: Vec<_> = error
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"))
        .collect();
    let [derive] = attrs.as_slice() else {
        return Err("expected one enum derive".into());
    };
    let paths = derive
        .parse_args_with(Punctuated::<syn::Path, Comma>::parse_terminated)
        .map_err(|e| e.to_string())?;
    let names: BTreeSet<_> = paths.iter().map(strict_path).collect::<Result<_>>()?;
    if !derive.path().is_ident("derive")
        || paths.len() != 2
        || names
            != BTreeSet::from([
                vec!["Debug".into()],
                vec!["thiserror".into(), "Error".into()],
            ])
        || !matches!(error.vis, Visibility::Public(_))
        || !error.generics.params.is_empty()
        || error.generics.where_clause.is_some()
    {
        return Err("expected ordinary public Debug/thiserror enum".into());
    }
    Ok(())
}

fn extract(source: &str, selection: Selection) -> Result<(Graph, Overlay)> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    if !docs(&file.attrs) || file.shebang.is_some() {
        return Err("unsupported parent attributes on error module".into());
    }
    let mut error = None;
    let mut alias = None;
    let mut helpers = BTreeMap::new();
    for item in &file.items {
        match item {
            Item::Enum(item) if item.ident == selection.error && error.is_none() => {
                error = Some(item)
            }
            Item::Type(item) if item.ident == selection.result && alias.is_none() => {
                alias = Some(item)
            }
            Item::Fn(item) if helpers.insert(item.sig.ident.to_string(), item).is_none() => {}
            _ => return Err("unexpected item in selected error module".into()),
        }
    }
    let (error, alias) = (
        error.ok_or("missing error enum")?,
        alias.ok_or("missing result alias")?,
    );
    derive_error(error)?;
    check_alias(alias, &error.ident)?;

    let id = selection.id;
    let fresh = |kind: &str, name: &str| format!("{id}:{kind}:{name}");
    let mut graph = Graph {
        format: "oddlang-finite-error-domain-v1".into(),
        id: id.clone(),
        error: fresh("type", &selection.error),
        result: fresh("alias", &selection.result),
        variants: BTreeMap::new(),
        categories: BTreeMap::new(),
    };
    let mut overlay = Overlay {
        format: "oddlang-finite-error-rust-v1".into(),
        names: BTreeMap::from([
            (graph.error.clone(), selection.error),
            (graph.result.clone(), selection.result),
        ]),
        variant_order: Vec::new(),
        category_types: BTreeMap::new(),
    };
    for variant in &error.variants {
        let attrs: Vec<_> = variant
            .attrs
            .iter()
            .filter(|attr| !attr.path().is_ident("doc"))
            .collect();
        let [attribute] = attrs.as_slice() else {
            return Err("expected one variant diagnostic".into());
        };
        if !attribute.path().is_ident("error") || variant.discriminant.is_some() {
            return Err("unsupported error variant attributes or discriminant".into());
        }
        let diagnostic = attribute
            .parse_args::<Diagnostic>()
            .map_err(|e| e.to_string())?;
        let mut source = false;
        let mut from = false;
        let payload = match &variant.fields {
            Fields::Unit if diagnostic.helper.is_none() => Payload::Unit,
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = fields.unnamed.first().unwrap();
                for attr in &field.attrs {
                    if !matches!(attr.meta, syn::Meta::Path(_)) {
                        return Err("unsupported payload attribute".into());
                    }
                    if attr.path().is_ident("source") && !source {
                        source = true;
                    } else if attr.path().is_ident("from") && !from {
                        from = true;
                    } else {
                        return Err("unsupported or repeated payload attribute".into());
                    }
                }
                source |= from;
                let ty = &field.ty;
                if let Some(name) = &diagnostic.helper {
                    let helper = helpers.get(name).ok_or("missing literal category helper")?;
                    let table = fresh("table", name);
                    let path = payload_path(ty)?;
                    if let Some(prior) = overlay.category_types.get(&table) {
                        if prior != &path {
                            return Err("category helper used for multiple enum types".into());
                        }
                    } else {
                        let members = literal_table(helper, ty)?;
                        let mut keys = BTreeMap::new();
                        for (member, text) in members {
                            let key = format!("{table}:member:{member}");
                            overlay.names.insert(key.clone(), member);
                            keys.insert(key, text);
                        }
                        graph.categories.insert(table.clone(), Table(keys));
                        overlay.category_types.insert(table.clone(), path);
                        overlay.names.insert(table.clone(), name.clone());
                    }
                    Payload::Category { table }
                } else if *ty == parse_quote!(String) {
                    Payload::OwnedText
                } else if *ty == parse_quote!(&'static str) {
                    Payload::StaticText
                } else if *ty == parse_quote!(std::io::Error) {
                    Payload::IoError
                } else {
                    return Err("unsupported error payload".into());
                }
            }
            _ => return Err("expected unit or single-field tuple error".into()),
        };
        let message = message(&diagnostic, !matches!(payload, Payload::Unit))?;
        let name = variant.ident.to_string();
        let key = fresh("variant", &name);
        overlay.names.insert(key.clone(), name);
        overlay.variant_order.push(key.clone());
        if graph
            .variants
            .insert(
                key,
                Variant {
                    payload,
                    message,
                    source,
                    from,
                },
            )
            .is_some()
        {
            return Err("repeated error variant".into());
        }
    }
    if helpers.len() != graph.categories.len() {
        return Err("unused function in selected error module".into());
    }
    validate(&graph, &overlay)?;
    Ok((graph, overlay))
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let source = io(fs::read_to_string(source))?;
    let selection_bytes = io(fs::read(selection))?;
    let selection: Selection =
        serde_json::from_slice(&selection_bytes).map_err(|e| e.to_string())?;
    let source_sha = digest(source.as_bytes());
    if selection.format != "oddlang-finite-error-selection-v1"
        || selection.source_sha256 != source_sha
    {
        return Err("finite error source selection version or digest mismatch".into());
    }
    let (graph, overlay) = extract(&source, selection)?;
    let graph = json(&graph)?;
    let overlay = json(&overlay)?;
    let receipt = json(&serde_json::json!({
        "format": "oddlang-finite-error-observation-v1",
        "sourceSha256": source_sha,
        "selectionSha256": digest(&selection_bytes),
        "graphSha256": digest(&graph),
        "overlaySha256": digest(&overlay),
        "gaps": ["thiserror expansion and caller-visible behavior require independent native replay",
                 "external enum exhaustiveness and crate-root reexports require downstream compilation"],
    }))?;
    publish(
        output,
        &[
            ("graph.json", &graph),
            ("rust.json", &overlay),
            ("observation.json", &receipt),
        ],
    )
}
