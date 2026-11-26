use super::*;
use syn::{Expr, ItemFn, Pat, Stmt, Token, parse::Parse, parse::ParseStream};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prefix {
    pub(super) allowed: Vec<String>,
    pub(super) denied: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    allow: String,
    route: String,
    error: String,
    supplied: String,
    item: String,
}

pub(super) fn recover(source: &str, selection: &[u8]) -> Result<Prefix, String> {
    let selection: Selection =
        serde_json::from_slice(selection).map_err(|error| error.to_string())?;
    let source = syn::parse_file(source).map_err(|error| error.to_string())?;
    let selected = |name: &str| -> Result<&ItemFn, String> {
        let found = source
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Fn(function) if function.sig.ident == name => Some(function),
                _ => None,
            })
            .collect::<Vec<_>>();
        match found.as_slice() {
            [function]
                if function
                    .attrs
                    .iter()
                    .all(|attribute| attribute.path().is_ident("doc")) =>
            {
                Ok(*function)
            }
            _ => Err(format!("expected one unconditional function {name}")),
        }
    };
    let allow = selected(&selection.allow)?;
    let [Stmt::Expr(Expr::Macro(expression), None)] = allow.block.stmts.as_slice() else {
        return Err("expected a literal allowlist predicate".into());
    };
    if !expression.mac.path.is_ident("matches") || !expression.attrs.is_empty() {
        return Err("expected a literal allowlist predicate".into());
    }
    let Matches(parameter, alternatives) =
        expression.mac.parse_body().map_err(|e| format!("{e}"))?;
    let expected: syn::Signature = syn::parse_quote!(fn allowed(#parameter: &str) -> bool);
    let mut actual = allow.sig.clone();
    actual.ident = expected.ident.clone();
    if actual != expected {
        return Err("unexpected allowlist signature".into());
    }
    let alternatives = match alternatives {
        Pat::Or(pattern) => pattern.cases.into_iter().collect(),
        pattern => vec![pattern],
    };
    let mut allowed = Vec::new();
    for pattern in alternatives {
        let Pat::Lit(literal) = pattern else {
            return Err("expected literal allowed option identifiers".into());
        };
        let syn::Lit::Str(value) = literal.lit else {
            return Err("expected literal allowed option identifiers".into());
        };
        allowed.push(value.value());
    }
    allowed.sort();
    if allowed.windows(2).any(|values| values[0] == values[1]) {
        return Err("duplicate allowed option identifier".into());
    }

    let ident =
        |value: &str| syn::parse_str::<syn::Ident>(value).map_err(|error| error.to_string());
    let supplied = ident(&selection.supplied)?;
    let item = ident(&selection.item)?;
    let allow = ident(&selection.allow)?;
    let error = ident(&selection.error)?;
    let condition: Expr = syn::parse_quote!(#supplied.iter().any(|#item| !#allow(#item)));
    let route = selected(&selection.route)?;
    let checks = route
        .block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Expr(Expr::If(branch), _) if *branch.cond == condition => Some(branch),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [check] = checks.as_slice() else {
        return Err("expected one direct rejected-prefix branch".into());
    };
    let [Stmt::Expr(Expr::Return(rejection), Some(_))] = check.then_branch.stmts.as_slice() else {
        return Err("expected a direct rejected-prefix return".into());
    };
    let Some(Expr::Call(unwrap)) = rejection.expr.as_deref() else {
        return Err("expected a returned prefix diagnostic".into());
    };
    let Some(Expr::Call(call)) = unwrap.args.first() else {
        return Err("expected a returned prefix diagnostic".into());
    };
    let Some(Expr::Lit(message)) = call.args.first() else {
        return Err("expected a literal prefix diagnostic".into());
    };
    if !message.attrs.is_empty() {
        return Err("unexpected prefix diagnostic attributes".into());
    }
    let syn::Lit::Str(message) = &message.lit else {
        return Err("expected a literal prefix diagnostic".into());
    };
    if check.else_branch.is_some()
        || !check.attrs.is_empty()
        || !rejection.attrs.is_empty()
        || !unwrap.attrs.is_empty()
        || !call.attrs.is_empty()
        || *unwrap.func != syn::parse_quote!(Err)
        || *call.func != syn::parse_quote!(#error)
        || unwrap.args.len() != 1
        || call.args.len() != 1
    {
        return Err("expected a direct returned prefix diagnostic".into());
    }

    let helper = selected(&selection.error)?;
    let [Stmt::Expr(Expr::Call(raw), None)] = helper.block.stmts.as_slice() else {
        return Err("expected a raw diagnostic formatter".into());
    };
    let Some(Expr::Macro(formatter)) = raw.args.get(1) else {
        return Err("expected a raw diagnostic formatter".into());
    };
    let literal = formatter
        .mac
        .parse_body::<syn::LitStr>()
        .map_err(|error| error.to_string())?;
    let template = literal.value();
    let (prefix, rest) = template
        .split_once('{')
        .ok_or("missing diagnostic interpolation")?;
    let (argument, tail) = rest
        .split_once('}')
        .ok_or("unclosed diagnostic interpolation")?;
    let argument = ident(argument)?;
    let signature: syn::Signature =
        syn::parse_quote!(fn #error(#argument: &'static str) -> clap::Error);
    let expected: Expr =
        syn::parse_quote!(clap::Error::raw(ErrorKind::InvalidValue, format!(#literal)));
    if Expr::Call(raw.clone()) != expected
        || helper.sig != signature
        || tail != "\n"
        || prefix.contains('}')
        || prefix.chars().any(char::is_control)
        || message.value().chars().any(char::is_control)
    {
        return Err("unsupported raw diagnostic format".into());
    }
    Ok(Prefix {
        allowed,
        denied: format!("{prefix}{}", message.value()),
    })
}

struct Matches(syn::Ident, Pat);

impl Parse for Matches {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let pattern = Pat::parse_multi(input)?;
        input.parse::<Option<Token![,]>>()?;
        Ok(Self(name, pattern))
    }
}
