//! Closed, syntax-directed frontend for the Rust ledger profile.
use proc_macro2::Span;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use syn::spanned::Spanned;
use syn::{BinOp, Expr, FnArg, Item, Pat, Stmt, Type};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub entity: String,
    pub entity_iri: String,
    pub operation: String,
    pub operation_iri: String,
    pub error_type: String,
    pub constructor: String,
    pub fields: BTreeMap<String, String>,
    pub args: BTreeMap<String, String>,
    pub guards: BTreeMap<String, String>,
    pub errors: BTreeMap<String, String>,
    pub min: i64,
    pub max: i64,
}

type R<T> = Result<T, String>;
type Locations = BTreeMap<String, [usize; 4]>;
const LEDGER: &str = "urn:oddlang:ledger:";
const ERROR: &str = "urn:oddlang:ledger:error:";

fn position(s: Span) -> [usize; 4] {
    let (a, b) = (s.start(), s.end());
    [a.line, a.column + 1, b.line, b.column + 1]
}
fn fail<T>(s: Span, reason: &str) -> R<T> {
    let [l, c, ..] = position(s);
    Err(format!("src/lib.rs:{l}:{c}: {reason}"))
}
fn need(ok: bool, s: Span, reason: &str) -> R<()> {
    if !ok {
        return fail(s, reason);
    }
    Ok(())
}
fn ident(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn ordinary(s: &str) -> bool {
    ident(s)
        && !matches!(
            s,
            "String"
                | "i64"
                | "i128"
                | "str"
                | "Option"
                | "Result"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
                | "std"
                | "Self"
        )
}
fn local(p: &Pat) -> Option<String> {
    let Pat::Ident(i) = p else { return None };
    (i.by_ref.is_none() && i.mutability.is_none() && i.subpat.is_none())
        .then(|| i.ident.to_string())
}
fn name(e: &Expr) -> Option<String> {
    let Expr::Path(p) = e else { return None };
    if p.qself.is_some() {
        return None;
    }
    p.path.get_ident().map(ToString::to_string)
}
fn path(e: &Expr, s: &str) -> bool {
    matches!(e, Expr::Path(p) if p.qself.is_none() && syn::parse_str::<syn::Path>(s).is_ok_and(|v| v == p.path))
}
fn method<'a>(e: &'a Expr, n: &str, argc: usize) -> Option<&'a syn::ExprMethodCall> {
    let Expr::MethodCall(m) = e else { return None };
    (m.method == n && m.turbofish.is_none() && m.args.len() == argc).then_some(m)
}
fn arg(a: &FnArg) -> R<(String, &Type)> {
    if let FnArg::Typed(t) = a
        && let Some(n) = local(&t.pat)
    {
        return Ok((n, &t.ty));
    }
    fail(a.span(), "expected a plain value argument")
}
fn type_is(t: &Type, expected: &str) -> bool {
    syn::parse_str::<Type>(expected).is_ok_and(|v| v == *t)
}
fn function(sig: &syn::Signature, result: &str, arity: usize) -> R<()> {
    let ok = sig.constness.is_none()
        && sig.asyncness.is_none()
        && sig.unsafety.is_none()
        && sig.abi.is_none()
        && sig.generics == Default::default()
        && sig.variadic.is_none()
        && sig.inputs.len() == arity
        && matches!(&sig.output, syn::ReturnType::Type(_,t) if type_is(t,result));
    need(ok, sig.span(), "unsupported function signature")
}
fn mapped(names: &BTreeMap<String, String>, suffix: &str) -> String {
    names
        .iter()
        .find(|(_, v)| **v == format!("{LEDGER}{suffix}"))
        .unwrap()
        .0
        .clone()
}
impl Mapping {
    fn check(&self) -> R<()> {
        let require = |ok: bool, reason: &str| {
            if ok {
                Ok(())
            } else {
                Err(format!("mapping.toml: {reason}"))
            }
        };
        require(
            self.min == 0 && self.max == i64::MAX,
            "unsupported constructor bounds",
        )?;
        require(
            [
                &self.entity,
                &self.error_type,
                &self.constructor,
                &self.operation,
            ]
            .iter()
            .all(|n| ordinary(n)),
            "unsupported mapped item name",
        )?;
        require(
            self.entity_iri == format!("{LEDGER}Account")
                && self.operation_iri == format!("{LEDGER}TransferFunds"),
            "unsupported entity or operation identity",
        )?;
        for (map, roles) in [
            (&self.fields, ["id", "owner", "balance"]),
            (&self.args, ["source", "destination", "amount"]),
        ] {
            require(
                map.len() == 3
                    && map.keys().all(|k| ordinary(k))
                    && roles
                        .iter()
                        .all(|n| map.values().any(|v| v == &format!("{LEDGER}{n}"))),
                "missing or ambiguous field or argument identity",
            )?;
        }
        require(
            !self.guards.is_empty() && self.guards.keys().eq(self.errors.keys()),
            "missing or stale guard/error identity",
        )?;
        let mut iris = BTreeSet::new();
        for iri in self
            .fields
            .values()
            .chain(self.args.values())
            .chain([&self.entity_iri, &self.operation_iri])
            .chain(self.guards.values())
            .chain(self.errors.values())
        {
            require(
                iris.insert(iri),
                &format!("duplicate mapped identity {iri}"),
            )?;
        }
        for (variant, iri) in &self.errors {
            let n = iri.strip_prefix(ERROR).unwrap_or("");
            require(
                ordinary(variant)
                    && ident(n)
                    && !matches!(n, "FieldOutOfRange" | "AssignmentConflict")
                    && self.guards[variant].strip_prefix(LEDGER).is_some_and(ident),
                "unsupported guard/error identity",
            )?;
        }
        Ok(())
    }
}
struct UnsupportedSyntax(Option<(Span, &'static str)>);
impl<'a> syn::visit::Visit<'a> for UnsupportedSyntax {
    fn visit_attribute(&mut self, a: &'a syn::Attribute) {
        self.0
            .get_or_insert((a.span(), "attributes are outside Rust import profile v1"));
    }
    fn visit_ident(&mut self, ident: &'a syn::Ident) {
        if !ident.to_string().is_ascii() {
            self.0.get_or_insert((
                ident.span(),
                "Rust import profile v1 requires ASCII identifiers",
            ));
        }
    }
}
pub fn lower(source: &str, m: &Mapping) -> R<(String, Locations)> {
    let file = syn::parse_file(source).map_err(|e| {
        format!(
            "src/lib.rs:{}:{}: invalid Rust syntax: {e}",
            e.span().start().line,
            e.span().start().column + 1
        )
    })?;
    let mut unsupported = UnsupportedSyntax(None);
    syn::visit::Visit::visit_file(&mut unsupported, &file);
    if let Some((span, reason)) = unsupported.0 {
        return fail(span, reason);
    }
    m.check()?;
    let mut loc = Locations::new();
    let mut function_item = None;
    let mut items = BTreeSet::new();
    for item in &file.items {
        let tag = match item {
            Item::Struct(v) if v.ident == m.entity => {
                need(
                    v.generics == Default::default()
                        && matches!(v.fields, syn::Fields::Named(_))
                        && v.fields.len() == 3,
                    v.span(),
                    "unsupported account structure",
                )?;
                loc.insert(m.entity_iri.clone(), position(v.ident.span()));
                for f in &v.fields {
                    let n = f.ident.as_ref().unwrap().to_string();
                    let Some(iri) = m.fields.get(&n) else {
                        return fail(f.span(), "unmapped account field");
                    };
                    need(
                        matches!(f.vis, syn::Visibility::Inherited)
                            && type_is(
                                &f.ty,
                                if iri == &format!("{LEDGER}balance") {
                                    "i64"
                                } else {
                                    "String"
                                },
                            ),
                        f.span(),
                        "expected private mapped field with primitive type",
                    )?;
                    loc.insert(iri.clone(), position(f.span()));
                }
                "account"
            }
            Item::Enum(v) if v.ident == m.error_type => {
                need(
                    v.generics == Default::default() && v.variants.len() == m.errors.len(),
                    v.span(),
                    "stale error mapping",
                )?;
                for var in &v.variants {
                    let Some(iri) = m.errors.get(&var.ident.to_string()) else {
                        return fail(var.span(), "unmapped error variant");
                    };
                    need(
                        matches!(var.fields, syn::Fields::Unit) && var.discriminant.is_none(),
                        var.span(),
                        "expected unit error variant",
                    )?;
                    loc.insert(iri.clone(), position(var.span()));
                }
                "errors"
            }
            Item::Impl(v) => {
                constructor(v, m)?;
                "constructor"
            }
            Item::Fn(f) if f.sig.ident == m.operation => {
                function_item = Some(f);
                "operation"
            }
            _ => {
                return fail(
                    item.span(),
                    "unsupported crate item in Rust import profile v1",
                );
            }
        };
        need(items.insert(tag), item.span(), "duplicate selected item")?;
    }
    need(items.len() == 4, file.span(), "missing selected item")?;
    Transfer::run(function_item.unwrap(), m, loc)
}
fn constructor(v: &syn::ItemImpl, m: &Mapping) -> R<()> {
    need(
        v.defaultness.is_none()
            && v.unsafety.is_none()
            && v.trait_.is_none()
            && v.generics == Default::default()
            && type_is(&v.self_ty, &m.entity)
            && v.items.len() == 1,
        v.span(),
        "unsupported account impl",
    )?;
    let syn::ImplItem::Fn(f) = &v.items[0] else {
        return fail(v.span(), "expected account constructor");
    };
    need(
        f.sig.ident == m.constructor && f.defaultness.is_none(),
        f.span(),
        "missing mapped constructor",
    )?;
    function(&f.sig, "Option<Self>", 3)?;
    let mut args = BTreeSet::new();
    let balance = mapped(&m.fields, "balance");
    for (a, role) in f.sig.inputs.iter().zip(["id", "owner", "balance"]) {
        let (n, t) = arg(a)?;
        need(
            n == mapped(&m.fields, role)
                && args.insert(n.clone())
                && type_is(t, if n == balance { "i64" } else { "String" }),
            a.span(),
            "expected constructor field argument",
        )?;
    }
    let [Stmt::Expr(Expr::If(g), _), Stmt::Expr(Expr::Call(c), None)] = f.block.stmts.as_slice()
    else {
        return fail(f.block.span(), "expected checked constructor");
    };
    let cond = syn::parse_str::<Expr>(&format!("{balance} < 0")).unwrap();
    let ret = syn::parse_str::<syn::Block>("{ return None; }").unwrap();
    need(
        *g.cond == cond && g.then_branch == ret && g.else_branch.is_none(),
        g.span(),
        "constructor disagrees with mapped balance bounds",
    )?;
    need(
        path(&c.func, "Some") && c.args.len() == 1,
        c.span(),
        "expected checked account result",
    )?;
    let Expr::Struct(s) = &c.args[0] else {
        return fail(c.span(), "expected account fields");
    };
    need(
        s.qself.is_none() && s.path.is_ident("Self") && s.rest.is_none() && s.fields.len() == 3,
        s.span(),
        "expected full account initialization",
    )?;
    for field in &s.fields {
        let syn::Member::Named(n) = &field.member else {
            return fail(field.span(), "expected named field");
        };
        need(
            args.remove(&n.to_string()) && name(&field.expr).as_deref() == Some(&n.to_string()),
            field.span(),
            "constructor must preserve field arguments",
        )?;
    }
    Ok(())
}

// A row denotes a*source_balance + b*destination_balance + c*amount + d <= 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Row([i128; 4]);
impl Row {
    fn constant(n: i128) -> Self {
        Self([0, 0, 0, n])
    }
    fn variable(i: usize) -> Self {
        let mut r = Self::constant(0);
        r.0[i] = 1;
        r
    }
    fn combine(self, lhs: i128, other: Self, rhs: i128) -> Option<Self> {
        let mut r = Self::constant(0);
        for (i, n) in r.0.iter_mut().enumerate() {
            *n = self.0[i]
                .checked_mul(lhs)?
                .checked_add(other.0[i].checked_mul(rhs)?)?;
        }
        Some(r)
    }
    fn at_most(self, other: Self, strict: bool) -> Option<Self> {
        self.combine(1, other, -1)?
            .combine(1, Self::constant(i128::from(strict)), 1)
    }
}
// An unsatisfiable relaxation is a certificate for the integral execution domain.
// Arithmetic overflow or excessive projection size means "unknown".
fn impossible(mut rows: BTreeSet<Row>) -> Option<bool> {
    for i in 0..3 {
        if rows.iter().any(|r| r.0[..3] == [0, 0, 0] && r.0[3] > 0) {
            return Some(true);
        }
        let mut out: BTreeSet<_> = rows.iter().copied().filter(|r| r.0[i] == 0).collect();
        for upper in rows.iter().filter(|r| r.0[i] > 0) {
            for lower in rows.iter().filter(|r| r.0[i] < 0) {
                out.insert(upper.combine(lower.0[i].checked_neg()?, *lower, upper.0[i])?);
                if out.len() > 256 {
                    return None;
                }
            }
        }
        rows = out;
    }
    Some(rows.iter().any(|r| r.0[3] > 0))
}
#[derive(Clone)]
struct Number {
    ttl: String,
    linear: Row,
    interval: (i128, i128),
    wide: bool,
    nodes: usize,
    depth: usize,
}
fn iri(n: &str) -> String {
    format!("<{n}>")
}
fn integer(n: i128) -> Number {
    Number {
        ttl: format!("[ a o:Constant ; o:integer {n} ]"),
        linear: Row::constant(n),
        interval: (n, n),
        wide: false,
        nodes: 1,
        depth: 1,
    }
}
fn binary(class: &str, l: &str, r: &str) -> String {
    format!("[ a o:{class} ; o:left {l} ; o:right {r} ]")
}
struct Transfer<'a> {
    m: &'a Mapping,
    map: String,
    balance: String,
    source: String,
    dest: String,
    amount: String,
    exist: BTreeSet<String>,
    distinct: bool,
    locals: BTreeMap<String, Number>,
    guards: BTreeMap<String, usize>,
    writes: BTreeMap<String, String>,
    proof: BTreeSet<Row>,
    ttl: String,
    loc: Locations,
}
impl<'a> Transfer<'a> {
    fn run(f: &syn::ItemFn, m: &'a Mapping, mut loc: Locations) -> R<(String, Locations)> {
        function(&f.sig, &format!("Result<(),{}>", m.error_type), 4)?;
        let args: Vec<_> = f.sig.inputs.iter().map(arg).collect::<R<_>>()?;
        let map = &args[0].0;
        need(
            ordinary(map)
                && !m.args.contains_key(map)
                && type_is(
                    args[0].1,
                    &format!("&mut std::collections::BTreeMap<String,{}>", m.entity),
                ),
            f.sig.inputs[0].span(),
            "expected a standard keyed account map",
        )?;
        let source = mapped(&m.args, "source");
        let dest = mapped(&m.args, "destination");
        let amount = mapped(&m.args, "amount");
        let mut ttl = String::from("@prefix o: <urn:oddlang:core:> .\n");
        let op = iri(&m.operation_iri);
        ttl.push_str(&format!("{op} a o:Operation .\n"));
        loc.insert(m.operation_iri.clone(), position(f.sig.ident.span()));
        for (idx, (n, t)) in args.iter().enumerate().skip(1) {
            let expected = [&source, &dest, &amount][idx - 1];
            need(
                n == expected && type_is(t, if idx == 3 { "i64" } else { "&str" }),
                f.sig.inputs[idx].span(),
                "stale argument mapping or unsupported argument type",
            )?;
            let id = &m.args[n];
            ttl.push_str(&format!(
                "{op} o:argument <{id}> .\n<{id}> a o:Argument ; o:type o:{} .\n",
                if idx == 3 { "I64" } else { "Text" }
            ));
            loc.insert(id.clone(), position(f.sig.inputs[idx].span()));
        }
        let mut proof = BTreeSet::new();
        for (i, (min, max)) in [(m.min, m.max), (m.min, m.max), (i64::MIN, i64::MAX)]
            .into_iter()
            .enumerate()
        {
            let var = Row::variable(i);
            proof.insert(Row::constant(min.into()).at_most(var, false).unwrap());
            proof.insert(var.at_most(Row::constant(max.into()), false).unwrap());
        }
        let mut this = Self {
            m,
            map: map.clone(),
            balance: mapped(&m.fields, "balance"),
            source,
            dest,
            amount,
            exist: Default::default(),
            distinct: false,
            locals: Default::default(),
            guards: Default::default(),
            writes: Default::default(),
            proof,
            ttl,
            loc,
        };
        let Some((last, body)) = f.block.stmts.split_last() else {
            return fail(f.span(), "empty transfer");
        };
        let success: syn::Expr = syn::parse_str("Ok(())").unwrap();
        need(
            matches!(last,Stmt::Expr(e,None) if *e==success),
            last.span(),
            "expected final Ok(())",
        )?;
        for stmt in body {
            match stmt {
                Stmt::Expr(Expr::Assign(a), Some(_)) => this.store(a)?,
                _ if !this.writes.is_empty() => {
                    return fail(
                        stmt.span(),
                        "only checked stores may follow the first write",
                    );
                }
                Stmt::Local(l) => {
                    let Some(n) = local(&l.pat) else {
                        return fail(l.span(), "expected immutable local");
                    };
                    need(
                        ordinary(&n)
                            && n != this.map
                            && !this.m.args.contains_key(&n)
                            && !this.locals.contains_key(&n),
                        l.span(),
                        "local shadowing or reserved name",
                    )?;
                    let Some(init) = &l.init else {
                        return fail(l.span(), "expected local initializer");
                    };
                    need(init.diverge.is_none(), l.span(), "unsupported let-else")?;
                    let value = this.number(&init.expr)?;
                    this.locals.insert(n, value);
                }
                Stmt::Expr(Expr::If(g), _) => this.guard(g)?,
                _ => return fail(stmt.span(), "unsupported transfer statement"),
            }
        }
        need(
            this.guards.len() == m.guards.len(),
            f.span(),
            "stale guard mapping",
        )?;
        need(
            this.writes.len() == 2,
            f.span(),
            "expected two distinct balance stores",
        )?;
        for line in this.writes.values() {
            this.ttl.push_str(line);
        }
        Ok((this.ttl, this.loc))
    }
    fn key(&self, e: &Expr) -> R<String> {
        if let Some(n) = name(e)
            && (n == self.source || n == self.dest)
        {
            return Ok(n);
        }
        fail(e.span(), "expected mapped account key")
    }
    fn number(&self, e: &Expr) -> R<Number> {
        let s = e.span();
        if let Some(n) = name(e) {
            if n == self.amount {
                return Ok(Number {
                    ttl: iri(&self.m.args[&n]),
                    linear: Row::variable(2),
                    interval: (i64::MIN.into(), i64::MAX.into()),
                    wide: false,
                    nodes: 1,
                    depth: 1,
                });
            }
            if let Some(v) = self.locals.get(&n) {
                return Ok(v.clone());
            }
        }
        match e {
            Expr::Paren(p) => self.number(&p.expr),
            Expr::Lit(l)
                if let syn::Lit::Int(n) = &l.lit
                    && n.suffix().is_empty() =>
            {
                let Ok(v) = n.base10_parse::<i128>() else {
                    return fail(s, "integer literal exceeds i128");
                };
                Ok(integer(v))
            }
            _ if path(e, "i64::MAX") => Ok(integer(i64::MAX.into())),
            Expr::Call(c) if path(&c.func, "i128::from") && c.args.len() == 1 => {
                let mut n = self.number(&c.args[0])?;
                n.wide = true;
                Ok(n)
            }
            Expr::Field(f) if matches!(&f.member,syn::Member::Named(n) if *n==self.balance) => {
                let Expr::Index(index) = &*f.base else {
                    return fail(s, "expected indexed snapshot read");
                };
                need(
                    name(&index.expr).as_ref() == Some(&self.map),
                    s,
                    "expected selected account map",
                )?;
                let key = self.key(&index.index)?;
                need(
                    self.exist.contains(&key),
                    s,
                    "snapshot read before successful existence guard",
                )?;
                Ok(Number {
                    ttl: format!(
                        "[ a o:Read ; o:account <{}> ; o:field <{}> ]",
                        self.m.args[&key], self.m.fields[&self.balance]
                    ),
                    linear: Row::variable(usize::from(key == self.dest)),
                    interval: (self.m.min.into(), self.m.max.into()),
                    wide: false,
                    nodes: 1,
                    depth: 1,
                })
            }
            Expr::Binary(b) if matches!(b.op, BinOp::Add(_) | BinOp::Sub(_)) => {
                let (l, r) = (self.number(&b.left)?, self.number(&b.right)?);
                need(
                    l.wide && r.wide,
                    s,
                    "arithmetic operands require i128 widening",
                )?;
                let nodes = 1 + l.nodes + r.nodes;
                let depth = 1 + l.depth.max(r.depth);
                need(
                    nodes <= 256 && depth <= 24,
                    s,
                    "expanded arithmetic exceeds ledger expression limits",
                )?;
                let add = matches!(b.op, BinOp::Add(_));
                let (lo, hi) = if add {
                    (
                        l.interval.0.checked_add(r.interval.0),
                        l.interval.1.checked_add(r.interval.1),
                    )
                } else {
                    (
                        l.interval.0.checked_sub(r.interval.1),
                        l.interval.1.checked_sub(r.interval.0),
                    )
                };
                let (Some(lo), Some(hi), Some(linear)) = (
                    lo,
                    hi,
                    l.linear.combine(1, r.linear, if add { 1 } else { -1 }),
                ) else {
                    return fail(s, "arithmetic exceeds i128 interval or proof limits");
                };
                Ok(Number {
                    ttl: binary(if add { "Add" } else { "Subtract" }, &l.ttl, &r.ttl),
                    linear,
                    interval: (lo, hi),
                    wide: true,
                    nodes,
                    depth,
                })
            }
            _ => fail(s, "unsupported integer expression"),
        }
    }
    fn guard(&mut self, g: &syn::ExprIf) -> R<()> {
        let s = g.span();
        need(g.else_branch.is_none(), s, "unsupported guard else branch")?;
        let [Stmt::Expr(Expr::Return(ret), Some(_))] = g.then_branch.stmts.as_slice() else {
            return fail(s, "expected single first-error return");
        };
        let Some(Expr::Call(call)) = ret.expr.as_deref() else {
            return fail(s, "expected Err(Error::Variant)");
        };
        need(
            path(&call.func, "Err") && call.args.len() == 1,
            s,
            "expected one named error",
        )?;
        let Some((variant, gid)) = self
            .m
            .guards
            .iter()
            .find(|(n, _)| path(&call.args[0], &format!("{}::{n}", self.m.error_type)))
        else {
            return fail(call.span(), "unmapped guard or error variant");
        };
        let priority = self.guards.len() + 1;
        need(
            self.guards.insert(variant.clone(), priority).is_none(),
            s,
            "duplicate guard identity",
        )?;
        let condition = match &*g.cond {
            Expr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) => {
                let Some(c) = method(&u.expr, "contains_key", 1) else {
                    return fail(s, "expected contains_key membership guard");
                };
                need(
                    name(&c.receiver).as_ref() == Some(&self.map),
                    s,
                    "expected selected account map",
                )?;
                let key = self.key(&c.args[0])?;
                self.exist.insert(key.clone());
                format!("[ a o:Exists ; o:account <{}> ]", self.m.args[&key])
            }
            Expr::Binary(b) if matches!(b.op, BinOp::Eq(_)) => {
                let (l, r) = (self.key(&b.left)?, self.key(&b.right)?);
                need(
                    l != r,
                    s,
                    "distinctness guard must compare different arguments",
                )?;
                self.distinct = true;
                binary(
                    "NotEqual",
                    &iri(&self.m.args[&self.source]),
                    &iri(&self.m.args[&self.dest]),
                )
            }
            Expr::Binary(b) => {
                let (l, r) = (self.number(&b.left)?, self.number(&b.right)?);
                // Success requires upper > lower or upper >= lower. Put both
                // spellings of the failed comparison into this same relation.
                let (upper, lower, strict) = match b.op {
                    BinOp::Le(_) => (&l, &r, true),
                    BinOp::Ge(_) => (&r, &l, true),
                    BinOp::Lt(_) => (&l, &r, false),
                    BinOp::Gt(_) => (&r, &l, false),
                    _ => return fail(s, "unsupported guard comparison"),
                };
                let Some(row) = lower.linear.at_most(upper.linear, strict) else {
                    return fail(s, "guard inequality exceeds proof limits");
                };
                self.proof.insert(row);
                if strict {
                    binary("Greater", &upper.ttl, &lower.ttl)
                } else if (upper.nodes, &upper.ttl) >= (lower.nodes, &lower.ttl) {
                    binary("GreaterEqual", &upper.ttl, &lower.ttl)
                } else {
                    binary("LessEqual", &lower.ttl, &upper.ttl)
                }
            }
            _ => return fail(g.cond.span(), "unsupported guard condition"),
        };
        let error = &self.m.errors[variant];
        self.ttl.push_str(&format!("<{}> o:guard <{gid}> .\n<{gid}> a o:Guard ; o:priority {priority} ; o:condition {condition} ; o:error <{error}> .\n<{error}> a o:Error .\n",self.m.operation_iri));
        self.loc.insert(gid.clone(), position(s));
        Ok(())
    }
    fn store(&mut self, a: &syn::ExprAssign) -> R<()> {
        let s = a.span();
        let Expr::Field(f) = &*a.left else {
            return fail(s, "expected balance store");
        };
        need(
            matches!(&f.member,syn::Member::Named(n) if *n==self.balance),
            s,
            "only balance stores are supported",
        )?;
        let Some(u) = method(&f.base, "unwrap", 0) else {
            return fail(s, "expected proven-present map update");
        };
        let Some(get) = method(&u.receiver, "get_mut", 1) else {
            return fail(s, "expected get_mut on selected map");
        };
        need(
            name(&get.receiver).as_ref() == Some(&self.map),
            s,
            "expected selected map in store",
        )?;
        let key = self.key(&get.args[0])?;
        need(
            self.distinct && self.exist.contains(&key) && !self.writes.contains_key(&key),
            s,
            "store requires proven presence and distinct targets",
        )?;
        let Expr::Cast(c) = &*a.right else {
            return fail(s, "expected checked local cast");
        };
        let Some(value) = name(&c.expr).and_then(|n| self.locals.get(&n)) else {
            return fail(s, "store must use a previously computed local");
        };
        need(
            type_is(&c.ty, "i64") && value.wide,
            s,
            "expected widened local narrowed to i64",
        )?;
        for outside in [
            value.linear.at_most(Row::constant(self.m.min.into()), true),
            Row::constant(self.m.max.into()).at_most(value.linear, true),
        ] {
            let Some(row) = outside else {
                return fail(s, "store range exceeds proof limits");
            };
            let mut assumptions = self.proof.clone();
            assumptions.insert(row);
            need(
                impossible(assumptions) == Some(true),
                s,
                "balance cast is not proven in range",
            )?;
        }
        self.writes.insert(
            self.m.args[&key].clone(),
            format!(
                "<{}> o:assign [ a o:Assign ; o:account <{}> ; o:field <{}> ; o:value {} ] .\n",
                self.m.operation_iri, self.m.args[&key], self.m.fields[&self.balance], value.ttl
            ),
        );
        Ok(())
    }
}
