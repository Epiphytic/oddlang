//! The standalone JSON codec profile and its portable observations.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use std::fmt::Write;

const TEXT: &str = "urn:oddlang:http:Text";
const BOOLEAN: &str = "urn:oddlang:http:Boolean";
const I64: &str = "urn:oddlang:http:I64";
const ROLE: &str = "json.codec";

fn parts(c: &HttpContract, f: &Field) -> (bool, bool, (i64, i64)) {
    let mut optional = false;
    let mut nullable = false;
    let mut bounds = (i64::MIN, i64::MAX);
    for k in &f.constraints {
        match c.constraints[k] {
            Constraint::Optional => optional = true,
            Constraint::Nullable => nullable = true,
            Constraint::Bounds { min, max } => bounds = (min, max),
            _ => {}
        }
    }
    (optional, nullable, bounds)
}
fn dependencies<'a>(c: &'a HttpContract, id: &Iri) -> Vec<&'a Iri> {
    if let Some(r) = c.records.get(id) {
        return r.fields.iter().map(|f| &c.fields[f].shape).collect();
    }
    match c.shapes.get(id) {
        Some(Shape::List(item)) => vec![item],
        Some(Shape::Union { variants, .. }) => variants.iter().collect(),
        Some(Shape::Variant { record, .. }) => vec![record],
        _ => vec![],
    }
}
fn valid_type(c: &HttpContract, i: &Iri) -> bool {
    matches!(i.as_str(), TEXT | BOOLEAN | I64)
        || c.records.contains_key(i)
        || matches!(c.shapes.get(i), Some(Shape::List(_) | Shape::Union { .. }))
}
pub(super) fn validate(c: &HttpContract) -> Result<(), HttpError> {
    // Each reachable shape must have a finite witness and a finite Rust type.
    fn visit<'a>(
        c: &'a HttpContract,
        id: &'a Iri,
        visiting: &mut BTreeSet<&'a Iri>,
        done: &mut BTreeSet<&'a Iri>,
    ) -> Result<(), HttpError> {
        if visiting.contains(id) {
            return Err(format!("{id}: cyclic codec shape"));
        }
        if !done.insert(id) {
            return Ok(());
        }
        visiting.insert(id);
        for child in dependencies(c, id) {
            visit(c, child, visiting, done)?;
        }
        visiting.remove(id);
        Ok(())
    }
    for (id, shape) in &c.shapes {
        match shape {
            Shape::List(item) if !valid_type(c, item) => {
                return Err(format!("{id}: unresolved list item {item}"));
            }
            Shape::Union {
                discriminator,
                variants,
            } => {
                if discriminator.is_empty() || variants.len() < 2 {
                    return Err(format!(
                        "{id}: expected discriminator and at least two variants"
                    ));
                }
                let mut tags = BTreeSet::new();
                for variant in variants {
                    let Some(Shape::Variant { tag, record }) = c.shapes.get(variant) else {
                        return Err(format!("{id}: unresolved union variant {variant}"));
                    };
                    let Some(record) = c.records.get(record) else {
                        return Err(format!("{variant}: unresolved variant record {record}"));
                    };
                    if tag.is_empty()
                        || !tags.insert(tag)
                        || record
                            .fields
                            .iter()
                            .any(|f| &c.fields[f].wire_name == discriminator)
                    {
                        return Err(format!("{variant}: empty or conflicting union tag"));
                    }
                }
            }
            Shape::Variant { record, .. } if !c.records.contains_key(record) => {
                return Err(format!("{id}: unresolved variant record {record}"));
            }
            _ => {}
        }
    }
    let mut done = BTreeSet::new();
    for id in c.records.keys().chain(c.shapes.keys()) {
        visit(c, id, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

pub(super) fn field_schema(
    c: &HttpContract,
    field: &Field,
    reference: &impl Fn(&Iri) -> Value,
) -> Value {
    let (_, nullable, (min, max)) = parts(c, field);
    let base = type_schema(&field.shape, (min, max), reference);
    if nullable {
        json!({"anyOf":[base,{"type":"null"}]})
    } else {
        base
    }
}
pub(super) fn required(c: &HttpContract, f: &Field) -> bool {
    !parts(c, f).0
}
fn type_schema(id: &Iri, (min, max): (i64, i64), reference: &impl Fn(&Iri) -> Value) -> Value {
    match id.as_str() {
        TEXT => json!({"type":"string"}),
        BOOLEAN => json!({"type":"boolean"}),
        I64 => {
            json!({"type":"string","pattern":"^(0|-?[1-9][0-9]*)$","x-decimal-range":[min.to_string(),max.to_string()]})
        }
        _ => reference(id),
    }
}
pub(super) fn schema_defs(
    c: &HttpContract,
    defs: &mut Map<String, Value>,
    reference: &impl Fn(&Iri) -> Value,
) {
    for (id, shape) in &c.shapes {
        let def = match shape {
            Shape::List(item) => {
                json!({"type":"array","items":type_schema(item,(i64::MIN,i64::MAX),reference)})
            }
            Shape::Union {
                discriminator,
                variants,
            } => {
                let items: Vec<_> = variants
                    .iter()
                    .map(|variant| {
                        let Shape::Variant { tag, record } = &c.shapes[variant] else {
                            unreachable!()
                        };
                        let mut def = defs[record.as_str()].clone();
                        def["properties"][discriminator] = json!({"const":tag});
                        def["required"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(discriminator));
                        def
                    })
                    .collect();
                json!({"oneOf":items})
            }
            Shape::Variant { .. } => continue,
        };
        defs.insert(id.to_string(), def);
    }
}

fn sample(c: &HttpContract, id: &Iri, bounds: (i64, i64)) -> Value {
    match id.as_str() {
        TEXT => json!("café"),
        BOOLEAN => json!(true),
        I64 => json!(0.max(bounds.0).min(bounds.1).to_string()),
        _ => match c.shapes.get(id) {
            Some(Shape::List(item)) => json!([sample(c, item, (i64::MIN, i64::MAX))]),
            Some(Shape::Union {
                discriminator,
                variants,
            }) => tagged(c, discriminator, variants.first().unwrap()),
            _ => Value::Object(
                c.records[id]
                    .fields
                    .iter()
                    .map(|f| {
                        let f = &c.fields[f];
                        (f.wire_name.clone(), sample(c, &f.shape, parts(c, f).2))
                    })
                    .collect(),
            ),
        },
    }
}
fn tagged(c: &HttpContract, discriminator: &str, variant: &Iri) -> Value {
    let Shape::Variant { tag, record } = &c.shapes[variant] else {
        unreachable!()
    };
    let mut payload = sample(c, record, (i64::MIN, i64::MAX));
    payload[discriminator] = json!(tag);
    payload
}
fn pointer(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}
fn set(root: &Value, path: &str, value: Value) -> Value {
    let mut result = root.clone();
    *result.pointer_mut(path).unwrap() = value;
    result
}
fn omit(root: &Value, parent: &str, key: &str) -> Value {
    let mut result = root.clone();
    result
        .pointer_mut(parent)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove(key);
    result
}

// Keep the surrounding valid document so the raw string alone determines rejection.
fn raw_text(root: &Value, path: &str, token: &[u8]) -> Vec<u8> {
    let original = root.to_string();
    let mut marker = "codec-witness".to_owned();
    while original.contains(&marker) {
        marker.push('_');
    }
    let marked = set(root, path, json!(marker)).to_string();
    let quoted = format!("\"{marker}\"");
    let (before, after) = marked.split_once(&quoted).unwrap();
    [before.as_bytes(), token, after.as_bytes()].concat()
}

fn duplicate_decoded_member(root: &Value) -> Option<Vec<u8>> {
    let (key, member) = root.as_object()?.iter().next()?;
    // Escape the first scalar so the duplicate has a distinct raw spelling.
    let ch = key.chars().next()?;
    let mut units = [0u16; 2];
    let first = ch
        .encode_utf16(&mut units)
        .iter()
        .map(|unit| format!("\\u{unit:04x}"))
        .collect::<String>();
    let rest = json!(&key[ch.len_utf8()..]).to_string();
    let body = root.to_string();
    let escaped_key = format!("\"{first}{}", &rest[1..]);
    Some(format!("{},{escaped_key}:{member}}}", &body[..body.len() - 1]).into_bytes())
}

struct Exercise<'a> {
    contract: &'a HttpContract,
    operation: &'a Iri,
    rows: Vec<Obligation>,
    cases: Vec<Case>,
}
impl Exercise<'_> {
    fn record_case(&mut self, id: &Iri, pos: &str, rule: &str, tests: Vec<(&str, Value, bool)>) {
        self.add_cases(
            id,
            pos,
            rule,
            tests.into_iter().map(|(kind, input, accepts)| {
                let bytes = serde_json::to_vec(&input).unwrap();
                let native = if accepts {
                    json!({"kind":"success","value":input})
                } else {
                    json!({"kind":"localRejection"})
                };
                (kind, bytes, native)
            }),
        );
    }
    fn raw_rejections(&mut self, id: &Iri, pos: &str, rule: &str, tests: Vec<(&str, Vec<u8>)>) {
        self.add_cases(
            id,
            pos,
            rule,
            tests
                .into_iter()
                .map(|(kind, body)| (kind, body, json!({"kind":"localRejection"}))),
        );
    }
    fn add_cases<'a>(
        &mut self,
        id: &Iri,
        pos: &str,
        rule: &str,
        cases: impl IntoIterator<Item = (&'a str, Vec<u8>, Value)>,
    ) {
        let tests: Vec<_> = cases.into_iter().collect();
        let obligation = Obligation {
            operation: self.operation.to_string(),
            role: ROLE.into(),
            declaration: id.to_string(),
            position: format!("body{pos}"),
            rule: format!("{H}rule:2:{rule}"),
            slots: tests.iter().map(|(kind, _, _)| (*kind).into()).collect(),
        };
        for (kind, bytes, native) in tests {
            let witness = obligation.witness(kind);
            let name: String = Sha256::digest(serde_json::to_vec(&witness).unwrap())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let body = STANDARD.encode(bytes);
            self.cases.push(Case {
                case_id: format!("codec/{name}"),
                operation: self.operation.to_string(),
                role: ROLE.into(),
                slots: vec![witness],
                fixture: json!([]),
                action: json!({"bodyBase64": body}),
                expected: vec![json!({"native":native})],
            });
        }
        self.rows.push(obligation);
    }
    fn walk(&mut self, id: &Iri, pos: &str, path: &str, root: &Value, bounds: (i64, i64)) {
        let c = self.contract;
        let value = root.pointer(path).unwrap();
        match id.as_str() {
            TEXT | BOOLEAN => {
                self.record_case(
                    id,
                    pos,
                    "scalar",
                    vec![
                        ("valid", root.clone(), true),
                        ("wrong-type", set(root, path, json!(17)), false),
                    ],
                );
                if id.as_str() == TEXT {
                    self.raw_rejections(
                        id,
                        pos,
                        "unicode",
                        vec![
                            ("isolated-surrogate", raw_text(root, path, br#""\uD800""#)),
                            ("invalid-utf8", raw_text(root, path, &[b'"', 0xff, b'"'])),
                        ],
                    );
                }
            }
            I64 => self.record_case(
                id,
                pos,
                "decimal",
                vec![
                    ("min", set(root, path, json!(bounds.0.to_string())), true),
                    ("max", set(root, path, json!(bounds.1.to_string())), true),
                    (
                        "outside",
                        set(root, path, json!((i128::from(bounds.1) + 1).to_string())),
                        false,
                    ),
                    ("noncanonical", set(root, path, json!("01")), false),
                    ("number-token", set(root, path, json!(bounds.0)), false),
                ],
            ),
            _ => match c.shapes.get(id) {
                Some(Shape::List(item)) => {
                    self.record_case(
                        id,
                        pos,
                        "list",
                        vec![
                            ("populated", root.clone(), true),
                            ("empty", set(root, path, json!([])), true),
                            ("wrong-type", set(root, path, json!({})), false),
                            (
                                "invalid-element",
                                set(root, &pointer(path, "0"), Value::Null),
                                false,
                            ),
                        ],
                    );
                    self.walk(
                        item,
                        &pointer(pos, "0"),
                        &pointer(path, "0"),
                        root,
                        (i64::MIN, i64::MAX),
                    );
                }
                Some(Shape::Union {
                    discriminator,
                    variants,
                }) => {
                    let mut tag = "unrecognized".to_owned();
                    while variants
                        .iter()
                        .any(|v| matches!(&c.shapes[v],Shape::Variant {tag:t,..} if t == &tag))
                    {
                        tag.push('_');
                    }
                    self.record_case(
                        id,
                        pos,
                        "tagged-union",
                        vec![
                            ("valid", root.clone(), true),
                            (
                                "unknown-tag",
                                set(root, &pointer(path, discriminator), json!(tag)),
                                false,
                            ),
                            ("missing-tag", omit(root, path, discriminator), false),
                        ],
                    );
                    for variant in variants {
                        let Shape::Variant { tag, record } = &c.shapes[variant] else {
                            unreachable!()
                        };
                        let variant_root = set(root, path, tagged(c, discriminator, variant));
                        let variant_pos = pointer(pos, &format!("@{tag}"));
                        self.record_case(
                            variant,
                            &variant_pos,
                            "variant",
                            vec![("selected", variant_root.clone(), true)],
                        );
                        self.walk(
                            record,
                            &variant_pos,
                            path,
                            &variant_root,
                            (i64::MIN, i64::MAX),
                        );
                    }
                }
                _ => {
                    let rec = &c.records[id];
                    self.record_case(
                        id,
                        pos,
                        "record",
                        vec![
                            ("valid", root.clone(), true),
                            (
                                "sequence",
                                set(
                                    root,
                                    path,
                                    Value::Array(
                                        rec.fields
                                            .iter()
                                            .map(|f| value[&c.fields[f].wire_name].clone())
                                            .collect(),
                                    ),
                                ),
                                false,
                            ),
                        ],
                    );
                    let mut unexpected = "unexpected".to_owned();
                    while value.get(&unexpected).is_some() {
                        unexpected.push('_');
                    }
                    let mut extra = value.clone();
                    extra[&unexpected] = json!(true);
                    for close in &rec.constraints {
                        self.record_case(
                            close,
                            pos,
                            "closed",
                            vec![
                                ("valid", root.clone(), true),
                                ("unknown-member", set(root, path, extra.clone()), false),
                            ],
                        );
                    }
                    for fid in &rec.fields {
                        let f = &c.fields[fid];
                        let fpos = pointer(pos, &f.wire_name);
                        let fpath = pointer(path, &f.wire_name);
                        let (optional, nullable, bounds) = parts(c, f);
                        self.record_case(
                            fid,
                            &fpos,
                            "field",
                            vec![
                                ("present", root.clone(), true),
                                ("omitted", omit(root, path, &f.wire_name), optional),
                                ("null", set(root, &fpath, Value::Null), nullable),
                            ],
                        );
                        for cid in &f.constraints {
                            let (rule, kind, input, accepts) = match c.constraints[cid] {
                                Constraint::Required => {
                                    ("required", "omitted", omit(root, path, &f.wire_name), false)
                                }
                                Constraint::Optional => {
                                    ("optional", "omitted", omit(root, path, &f.wire_name), true)
                                }
                                Constraint::Nullable => {
                                    ("nullable", "null", set(root, &fpath, Value::Null), true)
                                }
                                Constraint::Bounds { max, .. } => (
                                    "bounds",
                                    "outside",
                                    set(root, &fpath, json!((i128::from(max) + 1).to_string())),
                                    false,
                                ),
                                _ => unreachable!(),
                            };
                            self.record_case(cid, &fpos, rule, vec![(kind, input, accepts)]);
                        }
                        self.walk(&f.shape, &fpos, &fpath, root, bounds);
                    }
                }
            },
        }
    }
}

pub(super) fn exercise(c: &HttpContract) -> (Vec<Obligation>, Vec<Case>) {
    let mut rows = Vec::new();
    let mut cases = Vec::new();
    for (operation, op) in &c.operations {
        let mut e = Exercise {
            contract: c,
            operation,
            rows: Vec::new(),
            cases: Vec::new(),
        };
        let root = sample(c, &op.request, (i64::MIN, i64::MAX));
        e.record_case(
            operation,
            "",
            "codec",
            vec![("round-trip", root.clone(), true)],
        );
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(serde_json::to_vec(&root).unwrap());
        let mut wire = vec![("bom", bom)];
        if let Some(duplicate) = duplicate_decoded_member(&root) {
            wire.push(("duplicate-decoded-member", duplicate));
        }
        e.raw_rejections(operation, "", "json-bytes", wire);
        e.walk(&op.request, "", "", &root, (i64::MIN, i64::MAX));
        rows.extend(e.rows);
        cases.extend(e.cases);
    }
    rows.sort_by(|a, b| {
        (&a.operation, &a.position, &a.declaration, &a.rule).cmp(&(
            &b.operation,
            &b.position,
            &b.declaration,
            &b.rule,
        ))
    });
    cases.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    (rows, cases)
}

fn native_name(id: &Iri) -> Result<&str, HttpError> {
    let name = id.as_str().rsplit(':').next().unwrap();
    if !name.starts_with(|ch: char| ch.is_ascii_uppercase())
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || matches!(
            name,
            "CodecNullable"
                | "CodecDecimal"
                | "CodecObject"
                | "Self"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
        )
    {
        return Err(format!("{id}: unsupported native type name"));
    }
    Ok(name)
}

fn rust_type(c: &HttpContract, id: &Iri, (min, max): (i64, i64)) -> Result<String, HttpError> {
    Ok(match id.as_str() {
        TEXT => "String".into(),
        BOOLEAN => "bool".into(),
        I64 => format!("CodecDecimal<{min}i64,{max}i64>"),
        _ if c.records.contains_key(id)
            || matches!(c.shapes.get(id), Some(Shape::Union { .. })) =>
        {
            format!("CodecObject<{}>", native_name(id)?)
        }
        _ => native_name(id)?.into(),
    })
}
fn write_projection(
    c: &HttpContract,
    output: &Path,
    file: &str,
    contents: String,
) -> Result<(), HttpError> {
    if c.rule_set_version != 2 {
        return Err("standalone codec output requires rule set 2".into());
    }
    fs::create_dir_all(output)
        .and_then(|_| fs::write(output.join(file), contents))
        .map_err(|e| e.to_string())
}
pub fn emit_codec_rust(c: &HttpContract, output: &Path) -> Result<(), HttpError> {
    if c.rule_set_version != 2 {
        return Err("standalone codec output requires rule set 2".into());
    }
    let mut code = String::from(include_str!("shapes_rust.in"));
    let mut names = BTreeSet::new();
    for id in c.records.keys().chain(
        c.shapes
            .keys()
            .filter(|id| !matches!(c.shapes[*id], Shape::Variant { .. })),
    ) {
        if !names.insert(native_name(id)?) {
            return Err(format!("{id}: duplicate native type name"));
        }
    }
    for (id, rec) in &c.records {
        writeln!(code,"#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(deny_unknown_fields)]\n#[allow(non_snake_case)]\npub struct {} {{",native_name(id)?).unwrap();
        for fid in &rec.fields {
            let f = &c.fields[fid];
            let (optional, nullable, bounds) = parts(c, f);
            let mut ty = rust_type(c, &f.shape, bounds)?;
            if nullable {
                ty = format!("CodecNullable<{ty}>");
            }
            if optional {
                ty = format!("Option<{ty}>");
            }
            if matches!(f.native_name.as_str(), "self" | "Self" | "super" | "crate") {
                return Err(format!("{fid}: invalid Rust field name"));
            }
            if optional {
                code.push_str("#[serde(default, deserialize_with = \"present\", skip_serializing_if = \"Option::is_none\")]\n");
            } else if nullable {
                code.push_str("#[serde(deserialize_with = \"nullable\")]\n");
            }
            writeln!(
                code,
                "#[serde(rename = {:?})]\npub r#{}: {ty},",
                f.wire_name, f.native_name
            )
            .unwrap();
        }
        code.push_str("}\n");
    }
    for (id, shape) in &c.shapes {
        match shape {
            Shape::List(item) => writeln!(
                code,
                "pub type {} = Vec<{}>;",
                native_name(id)?,
                rust_type(c, item, (i64::MIN, i64::MAX))?
            )
            .unwrap(),
            Shape::Union {
                discriminator,
                variants,
            } => {
                writeln!(code,"#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(tag = {discriminator:?})]\npub enum {} {{",native_name(id)?).unwrap();
                let mut names = BTreeSet::new();
                for variant in variants {
                    let Shape::Variant { tag, record } = &c.shapes[variant] else {
                        unreachable!()
                    };
                    let name = native_name(variant)?;
                    if !names.insert(name) {
                        return Err(format!("{variant}: duplicate native variant name"));
                    }
                    writeln!(
                        code,
                        "#[serde(rename = {tag:?})]\n{name}({}),",
                        native_name(record)?
                    )
                    .unwrap();
                }
                code.push_str("}\n");
            }
            Shape::Variant { .. } => {}
        }
    }
    code.push_str("pub fn round_trip(operation: &str, body: &[u8]) -> Result<serde_json::Value, serde_json::Error> {\nmatch operation {\n");
    for (id, op) in &c.operations {
        writeln!(
            code,
            "{:?} => serde_json::to_value(serde_json::from_slice::<CodecObject<{}>>(body)?),",
            id.as_str(),
            native_name(&op.request)?
        )
        .unwrap();
    }
    code.push_str("_ => Err(serde::de::Error::custom(\"unknown codec operation\")),\n}\n}\n");
    write_projection(c, output, "codec.rs", code)
}
pub fn emit_codec_typescript(c: &HttpContract, output: &Path) -> Result<(), HttpError> {
    if c.rule_set_version != 2 {
        return Err("standalone codec output requires rule set 2".into());
    }
    let mut types = Map::new();
    for (id, rec) in &c.records {
        let fields: Map<_,_> = rec.fields.iter().map(|id| {
            let f = &c.fields[id];
            let (optional,nullable,(min,max)) = parts(c,f);
            (f.wire_name.clone(),json!({"type":f.shape,"optional":optional,"nullable":nullable,"min":min.to_string(),"max":max.to_string()}))
        }).collect();
        types.insert(id.to_string(), json!({"kind":"record","fields":fields}));
    }
    for (id, shape) in &c.shapes {
        let def = match shape {
            Shape::List(item) => json!({"kind":"list","item":item}),
            Shape::Union {
                discriminator,
                variants,
            } => {
                let rows: Vec<_> = variants
                    .iter()
                    .map(|v| {
                        let Shape::Variant { tag, record } = &c.shapes[v] else {
                            unreachable!()
                        };
                        json!([tag, record])
                    })
                    .collect();
                json!({"kind":"union","tag":discriminator,"variants":rows})
            }
            Shape::Variant { .. } => continue,
        };
        types.insert(id.to_string(), def);
    }
    let roots: Map<_, _> = c
        .operations
        .iter()
        .map(|(id, op)| (id.to_string(), json!(op.request)))
        .collect();
    let types = json!(Value::Object(types).to_string());
    let roots = json!(Value::Object(roots).to_string());
    let code = format!(
        "const shapes: Record<string, Shape> = JSON.parse({types});\nconst roots: Record<string,string> = JSON.parse({roots});\n{}",
        include_str!("shapes_typescript.in")
    );
    write_projection(c, output, "codec.ts", code)
}
