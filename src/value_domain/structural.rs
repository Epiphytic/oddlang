//! Closed lowering of optional unsigned newtype validation into interval facts.
use super::{Binding, Result};
use std::collections::BTreeMap;
use syn::{
    BinOp, Expr, Fields, FnArg, GenericArgument, ImplItem, ImplItemFn, Item, Lit, Pat,
    PathArguments, ReturnType, Stmt, Type, Visibility,
};

pub(super) struct Facts {
    pub(super) bits: u8,
    pub(super) range_error: String,
    pub(super) bounds: [u64; 3],
    pub(super) errors: BTreeMap<String, String>,
}

fn problem(context: &str) -> String {
    format!("unsupported optional interval source: {context}")
}

fn one<T>(values: impl IntoIterator<Item = T>, context: &str) -> Result<T> {
    let mut values = values.into_iter();
    let value = values.next().ok_or_else(|| problem(context))?;
    if values.next().is_some() {
        return Err(problem(context));
    }
    Ok(value)
}

fn name(expr: &Expr, name: &str) -> bool {
    matches!(strip(expr), Expr::Path(path) if path.qself.is_none() && path.path.is_ident(name))
}

fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(value) => strip(&value.expr),
        Expr::Group(value) => strip(&value.expr),
        value => value,
    }
}

fn declaration_type(ty: &Type, expected: &str) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.is_ident(expected))
}

fn arguments<'a>(
    ty: &'a Type,
    expected: &str,
) -> Option<&'a syn::punctuated::Punctuated<GenericArgument, syn::Token![,]>> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let first = path.path.segments.first()?;
    let PathArguments::AngleBracketed(arguments) = &first.arguments else {
        return None;
    };
    (first.ident == expected && arguments.colon2_token.is_none()).then_some(&arguments.args)
}

fn option_type(ty: &Type, primitive: &str) -> bool {
    matches!(arguments(ty, "Option").map(|v| v.iter().collect::<Vec<_>>()).as_deref(),
        Some([GenericArgument::Type(inner)]) if declaration_type(inner, primitive))
}

fn sole_expression(block: &syn::Block) -> Result<&Expr> {
    match block.stmts.as_slice() {
        [Stmt::Expr(value, None)] => Ok(value),
        _ => Err(problem("expected one expression")),
    }
}

fn call<'a>(expr: &'a Expr, target: &str) -> Result<&'a Expr> {
    let Expr::Call(call) = strip(expr) else {
        return Err(problem("expected wrapper call"));
    };
    if !name(&call.func, target) {
        return Err(problem("unexpected wrapper constructor"));
    }
    one(&call.args, "wrapper must have one value")
}

fn value(expr: &Expr, primitive: &str, maximum: u64, constant: Option<(&str, u64)>) -> Result<u64> {
    let resolved = match strip(expr) {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int) if int.suffix().is_empty() || int.suffix() == primitive => int
                .base10_parse::<u64>()
                .map_err(|_| problem("unsigned constant literal"))?,
            _ => return Err(problem("unsigned constant literal")),
        },
        Expr::Path(path) if path.qself.is_none() && path.path.leading_colon.is_none() => {
            let Some((name, value)) = constant else {
                return Err(problem("unsupported constant reference"));
            };
            let segments: Vec<_> = path.path.segments.iter().collect();
            if !matches!(segments.as_slice(), [first, second]
                if first.ident == "Self" && second.ident == name && matches!(first.arguments, PathArguments::None)
                && matches!(second.arguments, PathArguments::None))
            {
                return Err(problem("unsupported constant reference"));
            }
            value
        }
        Expr::Binary(bin) => {
            let left = value(&bin.left, primitive, maximum, constant)?;
            let right = value(&bin.right, primitive, maximum, constant)?;
            match bin.op {
                BinOp::Add(_) => left.checked_add(right),
                BinOp::Sub(_) => left.checked_sub(right),
                BinOp::Mul(_) => left.checked_mul(right),
                BinOp::Div(_) => left.checked_div(right),
                BinOp::Rem(_) => left.checked_rem(right),
                _ => return Err(problem("unsupported constant arithmetic")),
            }
            .ok_or_else(|| problem("constant arithmetic failure"))?
        }
        _ => return Err(problem("constant expression")),
    };
    if resolved > maximum {
        return Err(problem("unsigned constant overflow"));
    }
    Ok(resolved)
}

fn collect_rejections(
    expression: &Expr,
    parameter: &str,
    primitive: &str,
    width_maximum: u64,
    constant: (&str, u64),
    ranges: &mut Vec<(u64, u64)>,
) -> Result<()> {
    match strip(expression) {
        Expr::Binary(bin) if matches!(bin.op, BinOp::Or(_)) => {
            collect_rejections(
                &bin.left,
                parameter,
                primitive,
                width_maximum,
                constant,
                ranges,
            )?;
            collect_rejections(
                &bin.right,
                parameter,
                primitive,
                width_maximum,
                constant,
                ranges,
            )
        }
        Expr::Macro(item) if item.mac.path.is_ident("matches") => {
            struct MatchSome {
                value: Expr,
                _comma: syn::Token![,],
                pattern: Pat,
            }
            impl syn::parse::Parse for MatchSome {
                fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
                    Ok(Self {
                        value: input.parse()?,
                        _comma: input.parse()?,
                        pattern: Pat::parse_single(input)?,
                    })
                }
            }
            let syntax = item
                .mac
                .parse_body::<MatchSome>()
                .map_err(|_| problem("literal option match"))?;
            let Pat::TupleStruct(pattern) = syntax.pattern else {
                return Err(problem("literal option match"));
            };
            if !name(&syntax.value, parameter) || !pattern.path.is_ident("Some") {
                return Err(problem("literal option match"));
            }
            let Pat::Lit(inner) = one(&pattern.elems, "literal option match")? else {
                return Err(problem("literal option match"));
            };
            let value = value(
                &Expr::Lit(inner.clone()),
                primitive,
                width_maximum,
                Some(constant),
            )?;
            ranges.push((value, value));
            Ok(())
        }
        Expr::MethodCall(check)
            if check.method == "is_some_and"
                && check.turbofish.is_none()
                && name(&check.receiver, parameter) =>
        {
            let Expr::Closure(closure) = one(&check.args, "optional predicate")? else {
                return Err(problem("optional predicate"));
            };
            let Pat::Ident(variable) = one(&closure.inputs, "optional predicate argument")? else {
                return Err(problem("optional predicate argument"));
            };
            let Expr::Binary(compare) = strip(&closure.body) else {
                return Err(problem("optional comparison"));
            };
            if closure.asyncness.is_some()
                || closure.constness.is_some()
                || closure.movability.is_some()
                || variable.by_ref.is_some()
                || variable.mutability.is_some()
                || variable.subpat.is_some()
                || !name(&compare.left, &variable.ident.to_string())
            {
                return Err(problem("optional comparison"));
            }
            let boundary = value(&compare.right, primitive, width_maximum, Some(constant))?;
            let excluded = match compare.op {
                BinOp::Lt(_) => boundary.checked_sub(1).map(|last| (0, last)),
                BinOp::Le(_) => Some((0, boundary)),
                BinOp::Gt(_) => boundary
                    .checked_add(1)
                    .filter(|b| *b <= width_maximum)
                    .map(|first| (first, width_maximum)),
                BinOp::Ge(_) => Some((boundary, width_maximum)),
                BinOp::Eq(_) => Some((boundary, boundary)),
                _ => return Err(problem("optional comparison operator")),
            };
            ranges.extend(excluded);
            Ok(())
        }
        _ => Err(problem("constructor guard")),
    }
}

fn interval(mut rejected: Vec<(u64, u64)>, width: u64) -> Result<(u64, u64)> {
    rejected.sort_unstable();
    let mut accepted = Vec::new();
    let mut next = 0_u128;
    for (first, last) in rejected {
        let first = u128::from(first);
        if first > next {
            accepted.push((next as u64, (first - 1) as u64));
        }
        next = next.max(u128::from(last) + 1);
    }
    if next <= u128::from(width) {
        accepted.push((next as u64, width));
    }
    one(
        accepted,
        "valid values do not form exactly one closed interval",
    )
}

fn unconditional(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(|attr| {
        attr.path().get_ident().is_some_and(|ident| {
            matches!(
                ident.to_string().as_str(),
                "doc" | "derive" | "error" | "allow" | "deny" | "warn" | "forbid"
            )
        })
    })
}

pub(super) fn check_scope(file: &syn::File) -> Result<()> {
    fn reserved(name: &syn::Ident) -> bool {
        matches!(
            name.to_string().as_str(),
            "matches"
                | "Option"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
                | "Result"
                | "Default"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
        )
    }
    fn imported(tree: &syn::UseTree) -> bool {
        match tree {
            syn::UseTree::Name(name) => reserved(&name.ident),
            syn::UseTree::Rename(name) => reserved(&name.rename),
            syn::UseTree::Path(path) => reserved(&path.ident) || imported(&path.tree),
            syn::UseTree::Group(group) => group.items.iter().any(imported),
            syn::UseTree::Glob(_) => true,
        }
    }
    if file
        .attrs
        .iter()
        .any(|a| a.path().is_ident("no_implicit_prelude"))
        || file.items.iter().any(|item| match item {
            Item::Macro(_) | Item::Verbatim(_) => true,
            Item::Use(item) => imported(&item.tree),
            Item::Mod(item) => {
                reserved(&item.ident) || item.attrs.iter().any(|a| a.path().is_ident("macro_use"))
            }
            Item::ExternCrate(item) => {
                reserved(&item.ident)
                    || item.rename.as_ref().is_some_and(|(_, name)| reserved(name))
                    || item.attrs.iter().any(|a| a.path().is_ident("macro_use"))
            }
            Item::Type(item) => reserved(&item.ident),
            Item::Enum(item) => reserved(&item.ident),
            Item::Union(item) => reserved(&item.ident),
            Item::Struct(item) => reserved(&item.ident),
            Item::Trait(item) => reserved(&item.ident),
            Item::TraitAlias(item) => reserved(&item.ident),
            Item::Fn(item) => reserved(&item.sig.ident),
            Item::Const(item) => reserved(&item.ident),
            Item::Static(item) => reserved(&item.ident),
            _ => false,
        })
    {
        return Err(problem("unqualified prelude names may be shadowed"));
    }
    Ok(())
}

fn contains_active_attribute(node: &impl Fn(&mut AttributeCheck)) -> bool {
    let mut check = AttributeCheck(false);
    node(&mut check);
    check.0
}

struct AttributeCheck(bool);
impl<'ast> syn::visit::Visit<'ast> for AttributeCheck {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.0 |= !unconditional(std::slice::from_ref(attribute));
    }
}

fn method<'a>(implementation: &'a syn::ItemImpl, name: &str) -> Result<&'a ImplItemFn> {
    one(
        implementation.items.iter().filter_map(|item| match item {
            ImplItem::Fn(method) if method.sig.ident == name => Some(method),
            _ => None,
        }),
        name,
    )
    .and_then(|method| {
        let s = &method.sig;
        use syn::visit::Visit as _;
        if contains_active_attribute(&|visitor| visitor.visit_impl_item_fn(method))
            || !s.generics.params.is_empty()
            || s.generics.where_clause.is_some()
            || s.asyncness.is_some()
            || s.abi.is_some()
            || s.unsafety.is_some()
            || s.variadic.is_some()
        {
            return Err(problem(name));
        }
        Ok(method)
    })
}

pub(super) fn derive(source: &str, api: &Binding) -> Result<Facts> {
    let file =
        syn::parse_file(source).map_err(|err| format!("optional interval Rust parse: {err}"))?;
    check_scope(&file)?;
    let tuple = one(
        file.items.iter().filter_map(|item| match item {
            Item::Struct(item) if item.ident == api.type_name => Some(item),
            _ => None,
        }),
        "optional newtype",
    )?;
    let Fields::Unnamed(fields) = &tuple.fields else {
        return Err(problem("expected tuple newtype"));
    };
    let field = one(&fields.unnamed, "tuple field")?;
    let generics = arguments(&field.ty, "Option").ok_or_else(|| problem("optional type"))?;
    let GenericArgument::Type(Type::Path(inner)) = one(generics, "unsigned primitive")? else {
        return Err(problem("unsigned primitive"));
    };
    let primitive = inner
        .path
        .get_ident()
        .filter(|_| inner.qself.is_none())
        .ok_or_else(|| problem("unsigned primitive"))?
        .to_string();
    let bits = match primitive.as_str() {
        "u8" => 8,
        "u16" => 16,
        "u32" => 32,
        "u64" => 64,
        _ => return Err(problem("unsigned primitive")),
    };
    let width = u64::MAX >> (64 - bits);
    if !unconditional(&tuple.attrs)
        || !unconditional(&field.attrs)
        || !matches!(tuple.vis, Visibility::Public(_))
        || !matches!(field.vis, Visibility::Inherited)
        || !tuple.generics.params.is_empty()
        || tuple.generics.where_clause.is_some()
        || !option_type(&field.ty, &primitive)
    {
        return Err(problem("private optional unsigned field"));
    }
    let implementations = file.items.iter().filter_map(|item| match item {
        Item::Impl(item) if declaration_type(&item.self_ty, &api.type_name) => Some(item),
        _ => None,
    });
    let body = one(
        implementations.clone().filter(|i| i.trait_.is_none()),
        "inherent implementation",
    )?;
    let defaults = one(
        implementations.filter(|i| {
            i.trait_
                .as_ref()
                .is_some_and(|(neg, path, _)| neg.is_none() && path.is_ident("Default"))
        }),
        "default implementation",
    )?;
    for item in [body, defaults] {
        if !unconditional(&item.attrs)
            || !item.generics.params.is_empty()
            || item.generics.where_clause.is_some()
        {
            return Err(problem("conditional or generic implementation"));
        }
    }
    let maximum = one(
        body.items.iter().filter_map(|item| match item {
            ImplItem::Const(item) if item.ident == api.maximum_constant => Some(item),
            _ => None,
        }),
        "public maximum constant",
    )?;
    if !matches!(maximum.vis, Visibility::Public(_))
        || !unconditional(&maximum.attrs)
        || !declaration_type(&maximum.ty, &primitive)
    {
        return Err(problem("public maximum constant"));
    }
    use syn::visit::Visit as _;
    if contains_active_attribute(&|visitor| visitor.visit_expr(&maximum.expr)) {
        return Err(problem("conditional maximum expression"));
    }
    let maximum = value(&maximum.expr, &primitive, width, None)?;
    let constant = (api.maximum_constant.as_str(), maximum);
    let new = method(body, &api.constructor)?;
    let FnArg::Typed(input) = one(&new.sig.inputs, "constructor input")? else {
        return Err(problem("constructor input"));
    };
    let Pat::Ident(parameter) = input.pat.as_ref() else {
        return Err(problem("constructor binder"));
    };
    let ReturnType::Type(_, ret) = &new.sig.output else {
        return Err(problem("constructor result"));
    };
    let result = arguments(ret, "Result").map(|args| args.iter().collect::<Vec<_>>());
    if !matches!(result.as_deref(), Some([GenericArgument::Type(ok), GenericArgument::Type(err)])
        if declaration_type(ok, "Self") && declaration_type(err, &api.error_type))
        || !matches!(new.vis, Visibility::Public(_))
        || !option_type(&input.ty, &primitive)
        || parameter.mutability.is_some()
        || parameter.by_ref.is_some()
        || parameter.subpat.is_some()
    {
        return Err(problem("constructor signature"));
    }
    let [Stmt::Expr(Expr::If(guard), None), Stmt::Expr(success, None)] = new.block.stmts.as_slice()
    else {
        return Err(problem(
            "constructor must contain one guard and one success expression",
        ));
    };
    let parameter = parameter.ident.to_string();
    if guard.else_branch.is_some() || !name(call(call(success, "Ok")?, "Self")?, &parameter) {
        return Err(problem("constructor success must retain input"));
    }
    let [Stmt::Expr(Expr::Return(reject), Some(_))] = guard.then_branch.stmts.as_slice() else {
        return Err(problem("constructor must return one named error"));
    };
    let Expr::Path(error) = call(
        reject
            .expr
            .as_deref()
            .ok_or_else(|| problem("named error"))?,
        "Err",
    )?
    else {
        return Err(problem("named error"));
    };
    let segments: Vec<_> = error.path.segments.iter().collect();
    let [first, second] = segments.as_slice() else {
        return Err(problem("unexpected named error"));
    };
    if error.qself.is_some()
        || error.path.leading_colon.is_some()
        || first.ident != api.error_type
        || !matches!(first.arguments, PathArguments::None)
        || !matches!(second.arguments, PathArguments::None)
    {
        return Err(problem("unexpected named error"));
    }
    let range_error = api
        .variants
        .iter()
        .find(|(_, name)| second.ident == name.as_str())
        .map(|(id, _)| id.clone())
        .ok_or_else(|| problem("unbound named error"))?;
    let mut rejected = Vec::new();
    collect_rejections(
        &guard.cond,
        &parameter,
        &primitive,
        width,
        constant,
        &mut rejected,
    )?;
    let (minimum, recovered_maximum) = interval(rejected, width)?;
    if maximum != recovered_maximum {
        return Err(problem("published maximum and constructor differ"));
    }

    let accessor = method(body, &api.accessor)?;
    let FnArg::Receiver(receiver) = one(&accessor.sig.inputs, "accessor receiver")? else {
        return Err(problem("accessor receiver"));
    };
    let ReturnType::Type(_, output) = &accessor.sig.output else {
        return Err(problem("accessor result"));
    };
    if !matches!(accessor.vis, Visibility::Public(_))
        || !option_type(output, &primitive)
        || !matches!(receiver.reference, Some((_, None)))
        || receiver.mutability.is_some()
        || receiver.colon_token.is_some()
        || !matches!(sole_expression(&accessor.block)?, Expr::Field(value)
            if name(&value.base, "self") && matches!(&value.member, syn::Member::Unnamed(index) if index.index == 0))
    {
        return Err(problem("accessor must read field zero"));
    }
    let default = method(defaults, "default")?;
    if !default.sig.inputs.is_empty()
        || !matches!(&default.sig.output, ReturnType::Type(_, ty) if declaration_type(ty, "Self"))
    {
        return Err(problem("default signature"));
    }
    let default = value(
        call(call(sole_expression(&default.block)?, "Self")?, "Some")?,
        &primitive,
        width,
        Some(constant),
    )?;
    if !(minimum..=maximum).contains(&default) {
        return Err(problem("default outside the range"));
    }

    let declared = one(
        file.items.iter().filter_map(|item| match item {
            Item::Enum(item) if item.ident == api.error_type => Some(item),
            _ => None,
        }),
        "named error enum",
    )?;
    if !unconditional(&declared.attrs)
        || !matches!(declared.vis, Visibility::Public(_))
        || !declared.generics.params.is_empty()
        || declared.generics.where_clause.is_some()
        || declared.variants.len() != api.variants.len()
    {
        return Err(problem("named error enum shape"));
    }
    let mut errors = BTreeMap::new();
    for (index, variant) in declared.variants.iter().enumerate() {
        let (id, _) = api
            .variants
            .iter()
            .find(|(_, name)| variant.ident == name.as_str())
            .ok_or_else(|| problem("unbound error variant"))?;
        if !unconditional(&variant.attrs)
            || !matches!(variant.fields, Fields::Unit)
            || variant.discriminant.is_some()
            || api
                .variant_order
                .as_ref()
                .is_some_and(|order| order.get(index) != Some(id))
        {
            return Err(problem("named error variant shape"));
        }
        let message = one(
            variant
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("error")),
            "named error message",
        )?
        .parse_args::<syn::LitStr>()
        .map_err(|_| problem("literal error message"))?
        .value();
        if message.contains(['{', '}']) {
            return Err(problem("formatting error message"));
        }
        errors.insert(id.clone(), message);
    }
    Ok(Facts {
        bits,
        range_error,
        bounds: [minimum, maximum, default],
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
pub struct Quota(Option<u16>);
#[derive(Debug, Error)]
pub enum Rules {
    #[error("quota exceeded")]
    Exceeds,
    #[error("different failure")]
    Alternate,
}
impl Quota {
    pub const MAX: u16 = 10 * 30;
    pub fn from_count(count: Option<u16>) -> Result<Self, Rules> {
        if count.is_some_and(|n| n < 2) || count.is_some_and(|n| n > Self::MAX) { return Err(Rules::Exceeds); }
        Ok(Self(count))
    }
    pub fn count(&self) -> Option<u16> { self.0 }
}
impl Default for Quota {
    fn default() -> Self { Self(Some(10)) }
}
"#;

    fn fixture() -> Binding {
        serde_json::from_value(serde_json::json!({
            "domain": "urn:quota:range",
            "typeName": "Quota",
            "constructor": "from_count",
            "accessor": "count",
            "maximumConstant": "MAX",
            "errorType": "Rules",
            "variants": {"urn:quota:exceeds":"Exceeds","urn:quota:alternate":"Alternate"}
        }))
        .unwrap()
    }

    #[test]
    fn derives_width_bounds_and_actual_failure_path() {
        let facts = derive(SOURCE, &fixture()).unwrap();
        assert_eq!(facts.bits, 16);
        assert_eq!(facts.bounds, [2, 300, 10]);
        assert_eq!(facts.range_error, "urn:quota:exceeds");
        let modified = SOURCE
            .replace("10 * 30", "10 * 17")
            .replace("Rules::Exceeds", "Rules::Alternate");
        let revised = derive(&modified, &fixture()).unwrap();
        assert_eq!(revised.bounds, [2, 170, 10]);
        assert_eq!(revised.range_error, "urn:quota:alternate");
    }

    #[test]
    fn declines_non_interval_and_shadowed_source() {
        let single = SOURCE.replace(
            "count.is_some_and(|n| n < 2) || count.is_some_and(|n| n > Self::MAX)",
            "count.is_some_and(|n| n == 2)",
        );
        let error = derive(&single, &fixture()).err().unwrap();
        assert!(error.contains("exactly one closed interval"));
        let shadow = format!("macro_rules! matches {{ () => {{ false }} }} {SOURCE}");
        assert!(
            derive(&shadow, &fixture())
                .err()
                .unwrap()
                .contains("shadowed")
        );
    }
}
