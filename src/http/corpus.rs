//! Construct wire cases around independently authored successful examples.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Case {
    pub case_id: String,
    pub operation: String,
    pub role: String,
    pub slots: Vec<Slot>,
    pub fixture: Value,
    pub action: Value,
    pub expected: Vec<Value>,
}
pub(super) fn body(raw: &Value) -> Result<Value, String> {
    let b = STANDARD
        .decode(raw["bodyBase64"].as_str().ok_or("missing bodyBase64")?)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}
fn bytes(raw: &mut Value, b: &[u8]) {
    raw["bodyBase64"] = json!(STANDARD.encode(b));
}
fn put(raw: &mut Value, value: &Value) {
    bytes(raw, &serde_json::to_vec(value).unwrap());
}
fn response(status: u16, value: &Value) -> Value {
    let mut raw = json!({"status":status,"contentType":"application/json","bodyBase64":""});
    put(&mut raw, value);
    raw
}
fn component(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn template<'a>(sources: &'a [Case], s: &Slot, kind: &str) -> Result<&'a Case, String> {
    sources
        .iter()
        .find(|c| {
            c.operation == s.operation
                && c.role == s.role
                && c.slots
                    .iter()
                    .any(|t| t.position == "behavior" && t.kind == kind)
        })
        .ok_or_else(|| format!("missing authored {kind} scenario for {}", s.operation))
}
fn replace(v: &mut Value, old: &Value, new: &Value) {
    if v == old {
        *v = new.clone();
        return;
    }
    match v {
        Value::Array(a) => {
            for v in a {
                replace(v, old, new)
            }
        }
        Value::Object(m) => {
            for v in m.values_mut() {
                replace(v, old, new)
            }
        }
        _ => {}
    }
}
fn derive(model: &HttpContract, s: &Slot, sources: &[Case]) -> Result<Case, String> {
    let op = &model.operations[s.operation.as_str()];
    let get = op.method == "GET";
    let provider = s.role == "http.provider";
    let incoming = if provider { "request" } else { "response" };
    let rule = s.rule.rsplit(':').next().unwrap();
    let kind = s.kind.as_str();
    let src = template(sources, s, if get { "maximum" } else { "success" })?;
    let mut c = src.clone();
    c.case_id = format!(
        "rule/{}/{}/{}/{}/{}/{}",
        component(&s.operation),
        s.role,
        component(&s.declaration),
        rule,
        component(&s.position),
        kind
    );
    c.slots = vec![s.clone()];
    let mut req = src.expected[0]["request"].clone();
    let mut reply = src.expected[0]["response"].clone();
    let mut output = body(&reply)?;
    let mut input = if get {
        json!({"id":src.fixture[0]["id"]})
    } else {
        body(&req)?
    };
    let mut native = "success";
    let mut local = false;
    let mut transport = false;
    let mut failure = None;
    let mut raw_input = false;
    let mut raw_output = false;
    match rule {
        "route" if get && kind != "exact" => {
            let (id, wire) = match kind {
                "escaped" => ("café /A", "caf%C3%A9%20%2FA"),
                "plus" => ("A+B", if provider { "A+B" } else { "A%2BB" }),
                "bad-percent" => ("", "%XY"),
                "bad-utf8" => ("", "%FF"),
                "empty" => ("", ""),
                "dot" => (".", "."),
                "dot-dot" => ("..", ".."),
                _ => return Err(format!("unknown route kind {kind}")),
            };
            req["path"] = json!(op.path.replace("{id}", wire));
            input["id"] = json!(id);
            if kind.starts_with("bad-") {
                failure = Some("InvalidRequest");
            } else if ["empty", "dot", "dot-dot"].contains(&kind) {
                local = true;
                native = "localRejection";
            } else {
                c.fixture[0]["id"] = json!(id);
                output["id"] = json!(id);
            }
        }
        "media" => {
            let raw = if s.position == "request" {
                &mut req
            } else {
                &mut reply
            };
            raw["contentType"] = match kind {
                "charset" => json!("Application/JSON; Charset=UTF-8"),
                "missing" => Value::Null,
                "unsupported" => json!("text/plain"),
                _ => json!("application/json"),
            };
            if matches!(kind, "missing" | "unsupported") {
                if provider {
                    failure = Some("UnsupportedMediaType")
                } else {
                    native = "protocolError"
                }
            }
        }
        "json" if !matches!(kind, "valid") => {
            let raw = if s.position == "request" {
                raw_input = true;
                &mut req
            } else {
                raw_output = true;
                &mut reply
            };
            let original = STANDARD
                .decode(raw["bodyBase64"].as_str().unwrap())
                .unwrap();
            let data = match kind {
                "escaped" => {
                    let mut b = original.clone();
                    if let Some(i) = b.iter().position(u8::is_ascii_alphabetic) {
                        b.splice(i..=i, format!("\\u{:04X}", b[i]).bytes());
                    }
                    b
                }
                "empty" => vec![],
                "bom" => [&[0xef, 0xbb, 0xbf][..], &original].concat(),
                "utf8" => vec![0xff],
                "surrogate" => br#"{"x":"\uD800"}"#.to_vec(),
                "malformed" => b"{".to_vec(),
                "duplicate" => {
                    let v = body(raw)?;
                    let key = v
                        .as_object()
                        .and_then(|m| m.keys().next())
                        .ok_or("expected record")?;
                    let mut b = original.clone();
                    b.pop();
                    let escaped = format!("\\u{:04x}{}", key.as_bytes()[0], &key[1..]);
                    b.extend(format!(",\"{escaped}\":{}}}", v[key]).bytes());
                    b
                }
                "trailing" => [original.as_slice(), b" true"].concat(),
                _ => return Err(format!("unknown json kind {kind}")),
            };
            bytes(raw, &data);
            if kind != "escaped" {
                if provider {
                    failure = Some("InvalidRequest")
                } else {
                    native = "protocolError"
                }
            }
        }
        "response" if kind == "transport" => {
            transport = true;
            native = "protocolError";
        }
        "response" if kind == "unexpected-status" => {
            reply["status"] = json!(299);
            native = "protocolError";
        }
        "error" => {
            let status = model.errors[s.declaration.as_str()];
            if provider && kind == "named" {
                if !matches!(
                    s.declaration.as_str(),
                    "urn:oddlang:http:error:InvalidRequest"
                        | "urn:oddlang:http:error:UnsupportedMediaType"
                ) {
                    return Err(format!("missing authored named error witness {s:?}"));
                }
                if get {
                    req["path"] = json!(op.path.replace("{id}", "%FF"));
                } else if status == 415 {
                    req["contentType"] = Value::Null;
                } else {
                    raw_input = true;
                    bytes(&mut req, b"{");
                }
            }
            output = json!({"error":s.declaration});
            reply = response(status, &output);
            native = "namedError";
            if kind == "unknown" {
                output["error"] = json!("urn:example:unknown");
                native = "protocolError";
            }
            if kind == "malformed" {
                output = json!({"unexpected":true});
                native = "protocolError";
            }
        }
        "record" | "required" | "text" | "boolean" | "i64" | "bounds" => {
            let (pos, pointer) = s
                .position
                .split_once('/')
                .map(|(p, k)| (p, format!("/{k}")))
                .unwrap_or((&s.position, String::new()));
            let value = if pos == "request" {
                &mut input
            } else {
                &mut output
            };
            let field = value
                .pointer(&pointer)
                .cloned()
                .ok_or_else(|| format!("missing fixture field {pointer}"))?;
            let replacement = match kind {
                "omitted" => {
                    let (parent, key) = pointer.rsplit_once('/').unwrap();
                    value
                        .pointer_mut(parent)
                        .and_then(Value::as_object_mut)
                        .ok_or("parent is not object")?
                        .remove(&key.replace("~1", "/").replace("~0", "~"));
                    None
                }
                "unknown-member" => {
                    value.pointer_mut(&pointer).unwrap()["undeclared"] = json!(true);
                    None
                }
                "wrong-type" => Some(Value::Null),
                "noncanonical" => Some(json!("01")),
                "overflow" => Some(json!("9223372036854775808")),
                "numeric" => Some(json!(1)),
                "true" => Some(json!(true)),
                "false" => Some(json!(false)),
                "unicode" => Some(json!("Café e\u{301}/+")),
                "zero" => Some(json!("0")),
                "large" => Some(json!("9007199254740993")),
                "max" | "min" | "below" | "above" | "min-attainable" | "max-attainable" => {
                    let (min, max) = match model.constraints.get(s.declaration.as_str()) {
                        Some(Constraint::Bounds { min, max }) => (*min, *max),
                        _ => (i64::MIN, i64::MAX),
                    };
                    Some(json!(
                        match kind {
                            "max" => i128::from(max),
                            "min" => i128::from(min),
                            "below" => i128::from(min) - 1,
                            "min-attainable" => 1,
                            "max-attainable" => i128::from(i64::MAX) - 1,
                            _ => i128::from(max) + 1,
                        }
                        .to_string()
                    ))
                }
                _ => None,
            };
            let invalid = matches!(
                kind,
                "omitted"
                    | "unknown-member"
                    | "wrong-type"
                    | "noncanonical"
                    | "overflow"
                    | "numeric"
                    | "below"
                    | "above"
            );
            if let Some(new) = replacement {
                *value.pointer_mut(&pointer).unwrap() = new.clone();
                if !invalid && (provider || pos == "request") {
                    if rule == "text" {
                        replace(&mut c.fixture, &field, &new);
                        replace(&mut input, &field, &new);
                        replace(&mut output, &field, &new);
                    } else if !get && pos == "request" && pointer == "/amount" {
                        let n = new.as_str().unwrap().parse::<i64>().unwrap();
                        if n <= 0 {
                            let err = "urn:oddlang:ledger:error:NonPositiveAmount";
                            output = json!({"error":err});
                            reply = response(422, &output);
                            native = "namedError";
                        } else {
                            c.fixture[0]["balance"] = json!(n.to_string());
                            c.fixture[1]["balance"] = json!("0");
                            output["source"]["balance"] = json!("0");
                            output["destination"]["balance"] = new;
                        }
                    } else if get {
                        c.fixture[0]["balance"] = new;
                    } else if pos == "response" && pointer.ends_with("/balance") {
                        let source = pointer.starts_with("/source/");
                        let n = new.as_str().unwrap().parse::<i64>().unwrap();
                        let initial = if source {
                            n.checked_add(1)
                        } else {
                            n.checked_sub(1).filter(|x| *x >= 0)
                        };
                        if let Some(initial) = initial {
                            c.fixture[if source { 0 } else { 1 }]["balance"] =
                                json!(initial.to_string());
                            input["amount"] = json!("1");
                            let other = if source { "destination" } else { "source" };
                            output[other]["balance"] =
                                json!((if source { 8 } else { 9 }).to_string());
                        } else {
                            return Err(format!("unreachable transfer output at {pointer}: {n}"));
                        }
                    }
                }
            }
            if invalid && pos == incoming {
                if provider {
                    failure = Some("InvalidRequest")
                } else {
                    native = "protocolError"
                }
            }
            if get && pos == "request" && !invalid {
                req["path"] = json!(
                    op.path
                        .replace("{id}", &component(input["id"].as_str().unwrap()))
                )
            }
            if get && provider && pos == "response" {
                req["path"] = json!(
                    op.path
                        .replace("{id}", &component(c.fixture[0]["id"].as_str().unwrap()))
                )
            }
        }
        _ => {}
    }
    if let Some(name) = failure {
        output = json!({"error":format!("{H}error:{name}")});
        reply = response(
            if name == "UnsupportedMediaType" {
                415
            } else {
                400
            },
            &output,
        );
    }
    if !raw_input && !get {
        put(&mut req, &input);
    }
    if !raw_output {
        put(&mut reply, &output);
    }
    if transport {
        reply = Value::Null;
    }
    let mut exchanges = if local {
        vec![]
    } else {
        vec![json!({"request":req,"response":reply})]
    };
    if provider {
        if !get {
            for initial in c.fixture.as_array().ok_or("fixture must be array")? {
                let acct = if reply["status"] == 200 {
                    ["source", "destination"]
                        .iter()
                        .find_map(|k| {
                            (output[*k]["id"] == initial["id"]).then(|| output[*k].clone())
                        })
                        .unwrap_or_else(|| initial.clone())
                } else {
                    initial.clone()
                };
                let lookup = model
                    .operations
                    .values()
                    .find(|op| op.method == "GET")
                    .ok_or("missing account lookup")?;
                let request = json!({"method":"GET","path":lookup.path.replace("{id}",&component(acct["id"].as_str().ok_or("missing fixture id")?)),"contentType":null,"bodyBase64":""});
                exchanges.push(json!({"request":request,"response":response(200,&acct)}));
            }
        }
        c.action = json!({"requests":exchanges.iter().map(|e|&e["request"]).collect::<Vec<_>>()});
    } else {
        c.action = json!({"input":input,"replies":if local{vec![]}else{vec![reply]}});
        exchanges.push(json!({"native":match native{"success"=>json!({"kind":native,"value":output}),"namedError"=>json!({"kind":native,"error":output["error"]}),_=>json!({"kind":native})}}));
    }
    c.expected = exchanges;
    Ok(c)
}
pub fn cases(model: &HttpContract) -> Result<Vec<Case>, String> {
    let required = inventory(model);
    let slots: BTreeSet<_> = required
        .iter()
        .flat_map(|o| o.slots.iter().map(|k| o.witness(k)))
        .collect();
    let mut all = Vec::new();
    for file in &model.scenario_files {
        let authored: Vec<Case> = serde_json::from_slice(&model.source_files[file])
            .map_err(|e| format!("{file}: {e}"))?;
        for c in authored {
            if !super::wire::valid_case(&c) {
                return Err(format!("invalid authored exchanges {}", c.case_id));
            }
            for slot in &c.slots {
                if slot.rule.ends_with(":error") && slot.kind == "named" {
                    let reply = &c.expected[0]["response"];
                    if model
                        .errors
                        .get(slot.declaration.as_str())
                        .map(|s| u64::from(*s))
                        != reply["status"].as_u64()
                        || body(reply)? != json!({"error":slot.declaration})
                    {
                        return Err(format!("incorrect named error witness {}", c.case_id));
                    }
                }
            }
            if c.slots.is_empty()
                || c.slots
                    .iter()
                    .any(|s| s.role != c.role || s.operation != c.operation || !slots.contains(s))
            {
                return Err(format!("invalid scenario slots: {}", c.case_id));
            }
            all.push(c)
        }
    }
    let authored = all.clone();
    for s in gaps(&required, &authored) {
        if s.position == "behavior" {
            return Err(format!("missing authored witness {s:?}"));
        }
        all.push(derive(model, &s, &authored)?);
    }
    all.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    if all.windows(2).any(|p| p[0].case_id == p[1].case_id) {
        return Err("duplicate case ID".into());
    }
    Ok(all)
}
