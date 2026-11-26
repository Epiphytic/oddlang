use super::{Framing, Result, valid_message};
use proc_macro2::Span;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use syn::{parse_quote, spanned::Spanned, visit::Visit, *};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Coordinate {
    pub byte_start: usize,
    pub byte_end: usize,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}
impl From<Span> for Coordinate {
    fn from(s: Span) -> Self {
        Self {
            byte_start: s.byte_range().start,
            byte_end: s.byte_range().end,
            line: s.start().line,
            column: s.start().column + 1,
            end_line: s.end().line,
            end_column: s.end().column + 1,
        }
    }
}
pub(super) type Locations = BTreeMap<String, Coordinate>;

fn fail<T>(node: &impl Spanned, msg: &str) -> Result<T> {
    let s = node.span().start();
    Err(format!("src/frame.rs:{}:{}: {msg}", s.line, s.column + 1))
}
fn check<T: PartialEq + Spanned>(node: &T, expected: &T, label: &str) -> Result<()> {
    if node != expected {
        return fail(node, label);
    }
    Ok(())
}
fn drop_docs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|a| !literal_doc(a));
}
fn unit(name: &Ident, limits: bool) -> ItemStruct {
    if limits {
        parse_quote!(#[derive(Clone, Copy, Debug, Default)] pub struct #name;)
    } else {
        parse_quote!(pub struct #name;)
    }
}
fn unsigned(lit: &LitInt) -> Result<u64> {
    if !lit.suffix().is_empty() {
        return fail(lit, "expected unsuffixed integer");
    }
    lit.base10_parse()
        .map_err(|_| "framing integer out of range".into())
}
fn constant(e: &Expr) -> Result<u64> {
    match e {
        Expr::Lit(ExprLit {
            lit: Lit::Int(n),
            attrs,
        }) if attrs.is_empty() => unsigned(n),
        Expr::Binary(b) if b.attrs.is_empty() && matches!(b.op, BinOp::Mul(_)) => {
            constant(&b.left)?
                .checked_mul(constant(&b.right)?)
                .ok_or("framing bound overflows".into())
        }
        _ => fail(e, "expected unsigned literal or checked product"),
    }
}
fn binding(p: &Pat) -> Result<Ident> {
    match p {
        Pat::Ident(n)
            if n.by_ref.is_none()
                && n.mutability.is_none()
                && n.subpat.is_none()
                && n.attrs.is_empty() =>
        {
            Ok(n.ident.clone())
        }
        Pat::Type(t) => binding(&t.pat),
        _ => fail(p, "expected immutable local name"),
    }
}
fn local(s: &Stmt) -> Result<Ident> {
    if let Stmt::Local(l) = s {
        binding(&l.pat)
    } else {
        fail(s, "expected local binding")
    }
}
struct Numbers(Vec<LitInt>);
impl<'a> Visit<'a> for Numbers {
    fn visit_lit_int(&mut self, n: &'a LitInt) {
        self.0.push(n.clone());
    }
}
fn signature(f: &ImplItemFn) -> Result<Ident> {
    if !f.attrs.is_empty() || f.defaultness.is_some() || f.vis != parse_quote!(pub) {
        return fail(f, "unsupported method attributes or visibility");
    }
    let Some(FnArg::Typed(input)) = f.sig.inputs.first() else {
        return fail(f, "expected one byte slice argument");
    };
    let arg = binding(&input.pat)?;
    let name = &f.sig.ident;
    check(
        &f.sig,
        &parse_quote!(fn #name(#arg:&[u8])->Result<Vec<u8>,FrameError>),
        "unsupported framing method signature",
    )?;
    Ok(arg)
}
fn compare_steps(
    f: &ImplItemFn,
    expected: Vec<(&str, Stmt)>,
    locations: &mut Locations,
) -> Result<Vec<String>> {
    if f.block.stmts.len() != expected.len() {
        return fail(f, "unsupported framing statement count");
    }
    locations.insert(format!("{}.signature", f.sig.ident), f.sig.span().into());
    let mut ids = Vec::new();
    for (stmt, (id, pattern)) in f.block.stmts.iter().zip(expected) {
        check(
            stmt,
            &pattern,
            &format!("unsupported {} {id} statement", f.sig.ident),
        )?;
        locations.insert(format!("{}.{id}", f.sig.ident), stmt.span().into());
        if id != "encoded-bound" {
            ids.push(id.to_owned());
        }
    }
    Ok(ids)
}
fn encode(f: &ImplItemFn, locations: &mut Locations) -> Result<(u32, Vec<String>)> {
    let a = signature(f)?;
    let stmts = &f.block.stmts;
    if stmts.len() != 4 {
        return fail(f, "expected four encode statements");
    }
    let out = local(&stmts[1])?;
    let mut numbers = Numbers(vec![]);
    numbers.visit_stmt(&stmts[1]);
    let [version] = numbers.0.as_slice() else {
        return fail(&stmts[1], "expected one fixed u32 JSON version");
    };
    let steps = compare_steps(
        f,
        vec![
            (
                "raw-limit",
                parse_quote!(if #a.len()>Limits::RAW_BYTES {return Err(FrameError::SizeLimit);}),
            ),
            (
                "json-encode-base64",
                parse_quote!(let #out=serde_json::to_vec(&Envelope{version:#version,payload:STANDARD.encode(#a),}).map_err(|_|FrameError::InvalidJson)?;),
            ),
            (
                "json-limit",
                parse_quote!(if #out.len()>Limits::JSON_BYTES {return Err(FrameError::SizeLimit);}),
            ),
            ("return-frame", Stmt::Expr(parse_quote!(Ok(#out)), None)),
        ],
        locations,
    )?;
    Ok((
        unsigned(version)?
            .try_into()
            .map_err(|_| "expected a u32 version")?,
        steps,
    ))
}
fn decode(f: &ImplItemFn, version: u32, locations: &mut Locations) -> Result<Vec<String>> {
    let input = signature(f)?;
    let stmts = &f.block.stmts;
    if stmts.len() != 9 {
        return fail(f, "expected nine decode statements");
    }
    let record = local(&stmts[1])?;
    let bound_first = matches!(stmts[2], Stmt::Local(_));
    let bound_index = if bound_first { 2 } else { 3 };
    let bound = local(&stmts[bound_index])?;
    let bytes = local(&stmts[5])?;
    let version_expr = LitInt::new(&version.to_string(), Span::call_site());
    let version_step = (
        "version",
        parse_quote!(if #record.version != #version_expr {return Err(FrameError::InvalidVersion);}),
    );
    let mut steps = vec![
        (
            "json-limit",
            parse_quote!(if #input.len()>Limits::JSON_BYTES {return Err(FrameError::SizeLimit);}),
        ),
        (
            "json-decode-closed",
            parse_quote!(let #record:Envelope=serde_json::from_slice(#input).map_err(|_|FrameError::InvalidJson)?;),
        ),
    ];
    if !bound_first {
        steps.push(version_step.clone());
    }
    steps.push((
        "encoded-bound",
        parse_quote!(let #bound=Limits::RAW_BYTES.div_ceil(3)*4;),
    ));
    steps.push((
        "encoded-limit",
        parse_quote!(if #record.payload.len()>#bound{return Err(FrameError::SizeLimit);}),
    ));
    if bound_first {
        steps.push(version_step);
    }
    steps.extend([
        ("base64-decode",parse_quote!(let #bytes=STANDARD.decode(&#record.payload).map_err(|_|FrameError::InvalidEncoding)?;)),
        ("raw-limit",parse_quote!(if #bytes.len()>Limits::RAW_BYTES{return Err(FrameError::SizeLimit);})),
        ("canonical",parse_quote!(if STANDARD.encode(&#bytes)!=#record.payload{return Err(FrameError::InvalidEncoding);})),
        ("return-bytes",Stmt::Expr(parse_quote!(Ok(#bytes)),None)),
    ]);
    compare_steps(f, steps, locations)
}
fn constants(i: &ItemImpl, locations: &mut Locations) -> Result<[u64; 3]> {
    let mut values = BTreeMap::new();
    for c in &i.items {
        let ImplItem::Const(c) = c else {
            return fail(c, "unknown Limits member");
        };
        let id = &c.ident;
        let expr = &c.expr;
        check(
            c,
            &parse_quote!(pub const #id:usize=#expr;),
            "unsupported associated constant",
        )?;
        if values.insert(id.to_string(), constant(expr)?).is_some() {
            return fail(c, "duplicate bound");
        }
        locations.insert(format!("Limits.{id}"), c.span().into());
    }
    if values.len() != 3 {
        return fail(i, "expected three associated bounds");
    }
    Ok([
        *values.get("RAW_BYTES").ok_or("missing RAW_BYTES")?,
        *values.get("JSON_BYTES").ok_or("missing JSON_BYTES")?,
        *values.get("NDJSON_BYTES").ok_or("missing NDJSON_BYTES")?,
    ])
}
fn error_messages(e: &ItemEnum, locations: &mut Locations) -> Result<[String; 4]> {
    let mut header = e.clone();
    header.variants.clear();
    check(
        &header,
        &parse_quote!(
            #[derive(Debug, Error, PartialEq, Eq)]
            pub enum FrameError {}
        ),
        "unsupported FrameError derive or declaration",
    )?;
    let mut messages = vec![];
    for (i, v) in e.variants.iter().enumerate() {
        let name = &v.ident;
        if [
            "SizeLimit",
            "InvalidJson",
            "InvalidVersion",
            "InvalidEncoding",
        ]
        .get(i)
            != Some(&name.to_string().as_str())
        {
            return fail(v, "unknown error variant or order");
        }
        let [attr] = v.attrs.as_slice() else {
            return fail(v, "expected one error display attribute");
        };
        let msg: LitStr = attr
            .parse_args()
            .map_err(|_| "expected literal error message")?;
        check(
            v,
            &parse_quote!(#[error(#msg)] #name),
            "unsupported error attribute or payload",
        )?;
        if !valid_message(&msg.value()) {
            return fail(v, "unsupported display literal");
        }
        messages.push(msg.value());
        locations.insert(format!("FrameError.{name}"), v.span().into());
    }
    messages
        .try_into()
        .map_err(|_| "expected four frame errors".into())
}

pub(super) fn lower(source: &str) -> Result<(Framing, Locations)> {
    let file =
        parse_file(source).map_err(|e| format!("src/frame.rs:{}: {}", e.span().start().line, e))?;
    if file.shebang.is_some() || !file.attrs.is_empty() {
        return Err("unsupported framing file attributes".into());
    }
    let mut locations = Locations::new();
    let mut seen = BTreeSet::new();
    let mut bounds = None;
    let mut errors = None;
    let mut methods = BTreeMap::new();
    for item in file.items {
        let span = item.span();
        let id = match item {
            Item::Use(u) => {
                let allowed: [(&str, ItemUse); 4] = [
                    (
                        "base64",
                        parse_quote!(
                            use base64::{Engine, engine::general_purpose::STANDARD};
                        ),
                    ),
                    (
                        "base64",
                        parse_quote!(
                            use base64::{engine::general_purpose::STANDARD, Engine};
                        ),
                    ),
                    (
                        "serde",
                        parse_quote!(
                            use serde::{Deserialize, Serialize};
                        ),
                    ),
                    (
                        "thiserror",
                        parse_quote!(
                            use thiserror::Error;
                        ),
                    ),
                ];
                allowed
                    .into_iter()
                    .find(|(_, a)| *a == u)
                    .ok_or_else(|| {
                        format!(
                            "src/frame.rs:{}: unknown import or shadowing",
                            span.start().line
                        )
                    })?
                    .0
                    .to_owned()
            }
            Item::Struct(mut s) => {
                drop_docs(&mut s.attrs);
                let id = s.ident.to_string();
                match id.as_str() {
                    "ProbeEnvelope" | "Limits" => check(
                        &s,
                        &unit(&s.ident, id == "Limits"),
                        "unsupported unit struct or derive",
                    )?,
                    "Envelope" => check(
                        &s,
                        &parse_quote!(
                            #[derive(Serialize, Deserialize)]
                            #[serde(deny_unknown_fields)]
                            struct Envelope {
                                version: u32,
                                payload: String,
                            }
                        ),
                        "unsupported JSON fields, derive or serde attribute",
                    )?,
                    _ => return fail(&s, "unknown local type"),
                }
                id
            }
            Item::Enum(mut e) => {
                drop_docs(&mut e.attrs);
                errors = Some(error_messages(&e, &mut locations)?);
                "FrameError".to_owned()
            }
            Item::Impl(mut i) => {
                let mut header = i.clone();
                header.items.clear();
                if header == parse_quote!(impl Limits {}) {
                    bounds = Some(constants(&i, &mut locations)?);
                    "Limits.impl".into()
                } else if header == parse_quote!(impl ProbeEnvelope {}) {
                    for f in i.items.drain(..) {
                        let ImplItem::Fn(f) = f else {
                            return fail(&f, "expected codec method");
                        };
                        let name = f.sig.ident.to_string();
                        if !["encode", "decode"].contains(&name.as_str())
                            || methods.insert(name, f).is_some()
                        {
                            return Err("duplicate or unsupported codec method".into());
                        }
                    }
                    "ProbeEnvelope.impl".into()
                } else {
                    return fail(&i, "unknown impl or impl attributes");
                }
            }
            other => return fail(&other, "unsupported framing declaration"),
        };
        if !seen.insert(id.clone()) {
            return Err(format!("duplicate framing declaration {id}"));
        }
        locations.insert(id, span.into());
    }
    if seen.len() != 9 {
        return Err("missing required framing declaration".into());
    }
    let [raw_limit, json_limit, ndjson_limit] = bounds.ok_or("missing limits")?;
    let (version, encode_steps) = encode(
        methods.get("encode").ok_or("missing encode")?,
        &mut locations,
    )?;
    let decode_steps = decode(
        methods.get("decode").ok_or("missing decode")?,
        version,
        &mut locations,
    )?;
    let model = Framing {
        raw_limit,
        json_limit,
        ndjson_limit,
        version,
        encode_steps,
        decode_steps,
        errors: errors.ok_or("missing FrameError")?,
    };
    model.validate()?;
    Ok((model, locations))
}

pub(super) fn validate_locations(coords: &Locations, model: &Framing) -> Result<()> {
    let mut expected: BTreeSet<String> = [
        "base64",
        "serde",
        "thiserror",
        "Limits",
        "Limits.RAW_BYTES",
        "Limits.JSON_BYTES",
        "Limits.NDJSON_BYTES",
        "FrameError",
        "FrameError.SizeLimit",
        "FrameError.InvalidJson",
        "FrameError.InvalidVersion",
        "FrameError.InvalidEncoding",
        "ProbeEnvelope",
        "Envelope",
        "ProbeEnvelope.impl",
        "Limits.impl",
        "decode.encoded-bound",
        "encode.signature",
        "decode.signature",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (op, steps) in [
        ("encode", &model.encode_steps),
        ("decode", &model.decode_steps),
    ] {
        expected.extend(steps.iter().map(|s| format!("{op}.{s}")));
    }
    if expected.into_iter().ne(coords.keys().cloned())
        || coords.values().any(|v| {
            v.byte_start >= v.byte_end
                || v.line == 0
                || v.column == 0
                || v.end_column == 0
                || (v.end_line, v.end_column) < (v.line, v.column)
        })
    {
        return Err("invalid framing source coordinate inventory".into());
    }
    Ok(())
}

pub(super) fn literal_doc(attr: &Attribute) -> bool {
    if let Meta::NameValue(v) = &attr.meta {
        return v.path.is_ident("doc")
            && matches!(&v.value,Expr::Lit(ExprLit{lit:Lit::Str(_),attrs}) if attrs.is_empty());
    }
    false
}
