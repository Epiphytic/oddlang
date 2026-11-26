use super::*;
use syn::{
    Block, Expr, FnArg, Item, Lit, Pat, Stmt, Token, Type, Visibility,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
};

pub(super) fn extract(source: &str) -> Result<Graph> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    if !only_docs(&file.attrs) {
        return Err("unexpected diagnostic module attributes".into());
    }

    let io: syn::ItemUse = syn::parse_quote!(
        use std::io::Write;
    );
    let atomic: syn::ItemUse = syn::parse_quote!(
        use std::sync::atomic::{AtomicBool, Ordering};
    );
    let mut imports = (false, false);
    let mut flag = None;
    let mut functions = Vec::new();
    for item in file.items {
        match item {
            Item::Use(item) if item == io && !imports.0 => imports.0 = true,
            Item::Use(item) if item == atomic && !imports.1 => imports.1 = true,
            Item::Static(item)
                if flag.is_none()
                    && only_docs(&item.attrs)
                    && matches!(item.vis, Visibility::Inherited)
                    && matches!(item.mutability, syn::StaticMutability::None)
                    && *item.ty == syn::parse_quote!(AtomicBool) =>
            {
                let Expr::Call(init) = *item.expr else {
                    return Err("expected literal flag initialization".into());
                };
                if *init.func != syn::parse_quote!(AtomicBool::new) || init.args.len() != 1 {
                    return Err("expected literal flag initialization".into());
                }
                flag = Some(Flag {
                    name: item.ident.to_string(),
                    initial: boolean(&init.args[0])?,
                });
            }
            Item::Fn(item) => functions.push(item),
            _ => return Err("unsupported declaration or import in diagnostic module".into()),
        }
    }
    if imports != (true, true) {
        return Err("missing diagnostic imports".into());
    }
    let flag = flag.ok_or("missing diagnostic flag")?;
    let functions = functions
        .into_iter()
        .map(|item| function(item, &flag.name))
        .collect::<Result<Vec<_>>>()?;
    Ok(Graph {
        format: PROFILE.into(),
        flag,
        functions,
    })
}

fn only_docs(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .all(|attribute| attribute.path().is_ident("doc"))
}

fn function(item: syn::ItemFn, flag: &str) -> Result<Function> {
    let sig = item.sig;
    if !matches!(item.vis, Visibility::Public(_))
        || !only_docs(&item.attrs)
        || sig.constness.is_some()
        || sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
        || sig.variadic.is_some()
        || !matches!(sig.output, syn::ReturnType::Default)
    {
        return Err("expected an ordinary public diagnostic function".into());
    }
    Ok(Function {
        name: sig.ident.to_string(),
        parameters: sig.inputs.iter().map(parameter).collect::<Result<_>>()?,
        actions: block(&item.block, flag)?,
    })
}

fn parameter(input: &FnArg) -> Result<Parameter> {
    let FnArg::Typed(input) = input else {
        return Err("expected named text parameter".into());
    };
    let Pat::Ident(name) = input.pat.as_ref() else {
        return Err("expected named text parameter".into());
    };
    let readonly: Type = syn::parse_quote!(&'static str);
    let writable: Type = syn::parse_quote!(&mut &'static str);
    if !input.attrs.is_empty()
        || !name.attrs.is_empty()
        || name.by_ref.is_some()
        || name.mutability.is_some()
        || name.subpat.is_some()
        || (*input.ty != readonly && *input.ty != writable)
    {
        return Err("unsupported diagnostic text parameter".into());
    }
    Ok(Parameter {
        name: name.ident.to_string(),
        writable: *input.ty == writable,
    })
}

fn block(body: &Block, flag: &str) -> Result<Vec<Action>> {
    body.stmts
        .iter()
        .map(|statement| action(statement, flag))
        .collect()
}

fn action(statement: &Stmt, flag: &str) -> Result<Action> {
    match statement {
        Stmt::Expr(Expr::MethodCall(call), Some(_))
            if call.attrs.is_empty()
                && call.method == "store"
                && call.turbofish.is_none()
                && name(&call.receiver)? == flag
                && call.args.len() == 2
                && call.args[1] == syn::parse_quote!(Ordering::Relaxed) =>
        {
            Ok(Action::Set {
                value: boolean(&call.args[0])?,
            })
        }
        Stmt::Expr(Expr::Assign(assign), Some(_)) if assign.attrs.is_empty() => {
            let Expr::Unary(target) = assign.left.as_ref() else {
                return Err("expected direct writable parameter assignment".into());
            };
            if !target.attrs.is_empty() || !matches!(target.op, syn::UnOp::Deref(_)) {
                return Err("expected direct writable parameter assignment".into());
            }
            Ok(Action::Assign {
                target: name(&target.expr)?,
                value: text(&assign.right)?,
            })
        }
        Stmt::Expr(Expr::If(guard), _) if guard.attrs.is_empty() && guard.else_branch.is_none() => {
            let receiver: syn::Ident = syn::parse_str(flag).map_err(|error| error.to_string())?;
            let condition: Expr = syn::parse_quote!(#receiver.load(Ordering::Relaxed));
            if *guard.cond != condition {
                return Err("expected a flag-controlled block".into());
            }
            Ok(Action::When {
                actions: block(&guard.then_branch, flag)?,
            })
        }
        Stmt::Expr(Expr::Call(call), Some(_)) if call.attrs.is_empty() => Ok(Action::Call {
            function: name(&call.func)?,
            arguments: call.args.iter().map(text).collect::<Result<_>>()?,
        }),
        Stmt::Local(local) if local.attrs.is_empty() && local.pat == syn::parse_quote!(_) => {
            let init = local
                .init
                .as_ref()
                .filter(|init| init.diverge.is_none())
                .ok_or("expected an ignored writer result")?;
            emit(&init.expr)
        }
        _ => Err("unsupported diagnostic statement".into()),
    }
}

fn name(expr: &Expr) -> Result<String> {
    if let Expr::Path(path) = expr {
        if path.attrs.is_empty() && path.qself.is_none() {
            if let Some(ident) = path.path.get_ident() {
                return Ok(ident.to_string());
            }
        }
    }
    Err("expected an unqualified diagnostic name".into())
}

fn boolean(expr: &Expr) -> Result<bool> {
    match expr {
        Expr::Lit(value) if value.attrs.is_empty() => match &value.lit {
            Lit::Bool(value) => Ok(value.value),
            _ => Err("expected a boolean literal".into()),
        },
        _ => Err("expected a boolean literal".into()),
    }
}

fn text(expr: &Expr) -> Result<Text> {
    if let Expr::Lit(value) = expr {
        if value.attrs.is_empty() {
            if let Lit::Str(value) = &value.lit {
                return Ok(Text::Literal {
                    value: value.value(),
                });
            }
        }
    }
    name(expr).map(|name| Text::Parameter { name })
}

fn expr_macro<'a>(expr: &'a Expr, expected: syn::Path) -> Result<&'a syn::Macro> {
    if let Expr::Macro(expr) = expr {
        if expr.attrs.is_empty() && expr.mac.path == expected {
            return Ok(&expr.mac);
        }
    }
    Err("unexpected diagnostic writer or record provider".into())
}

fn emit(expr: &Expr) -> Result<Action> {
    let writer = expr_macro(expr, syn::parse_quote!(writeln))?;
    let args = Punctuated::<Expr, Token![,]>::parse_terminated
        .parse2(writer.tokens.clone())
        .map_err(|error| error.to_string())?;
    let sink: Expr = syn::parse_quote!(std::io::stderr().lock());
    let format: Expr = syn::parse_quote!("{}");
    if args.len() != 3 || args[0] != sink || args[1] != format {
        return Err("expected a single JSON line written to the locked error stream".into());
    }
    let record = expr_macro(&args[2], syn::parse_quote!(serde_json::json))?;
    let Record(fields) = record.parse_body().map_err(|error| error.to_string())?;
    Ok(Action::Emit { fields })
}

struct Record(BTreeMap<String, Value>);

impl Parse for Record {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::braced!(content in input);
        let mut fields = BTreeMap::new();
        while !content.is_empty() {
            let key: syn::LitStr = content.parse()?;
            content.parse::<Token![:]>()?;
            let expr: Expr = content.parse()?;
            let value = value(&expr).map_err(|error| syn::Error::new_spanned(expr, error))?;
            if fields.insert(key.value(), value).is_some() {
                return Err(syn::Error::new(key.span(), "duplicate record field"));
            }
            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }
        }
        Ok(Self(fields))
    }
}

fn value(expr: &Expr) -> Result<Value> {
    if let Expr::Lit(literal) = expr {
        if literal.attrs.is_empty() {
            match &literal.lit {
                Lit::Int(number) if matches!(number.suffix(), "" | "u64") => {
                    let value = number
                        .base10_parse::<u64>()
                        .map_err(|error| error.to_string())?;
                    if number.suffix().is_empty() && value > i32::MAX as u64 {
                        return Err("out-of-range unsuffixed record integer".into());
                    }
                    return Ok(Value::Unsigned { value });
                }
                Lit::Bool(value) => return Ok(Value::Boolean { value: value.value }),
                _ => (),
            }
        }
    }
    let host_os: Expr = syn::parse_quote!(std::env::consts::OS);
    let host_arch: Expr = syn::parse_quote!(std::env::consts::ARCH);
    let version: Expr = syn::parse_quote!(env!("CARGO_PKG_VERSION"));
    if expr == &host_os {
        Ok(Value::HostOs)
    } else if expr == &host_arch {
        Ok(Value::HostArchitecture)
    } else if expr == &version {
        Ok(Value::PackageVersion)
    } else {
        text(expr).map(|source| Value::Text { source })
    }
}
