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
fn duplicate_member(record: &Value, key: &str) -> Result<Vec<u8>, String> {
    let mut b = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    b.pop();
    let escaped = key
        .encode_utf16()
        .map(|u| format!("\\u{u:04x}"))
        .collect::<String>();
    b.extend(format!(",\"{escaped}\":{}}}", record[key]).bytes());
    Ok(b)
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
// Keep an account ID coherent across the native input, success output and
// initial state. Other text fields can have the same value as that ID.
fn set_account_text(
    fixture: &mut Value,
    input: &mut Value,
    output: &mut Value,
    position: &str,
    pointer: &str,
    old: &Value,
    new: &Value,
) -> Result<(), String> {
    let account_key = if position == "request" {
        pointer.trim_start_matches('/')
    } else {
        pointer.split('/').nth(1).unwrap_or_default()
    };
    let pair = output.get("source").is_some();
    let owner = pointer.ends_with("/owner");
    let id = if owner {
        if pair {
            output[account_key]["id"].clone()
        } else {
            output["id"].clone()
        }
    } else {
        old.clone()
    };
    let Some(account) = fixture
        .as_array_mut()
        .and_then(|a| a.iter_mut().find(|v| v["id"] == id))
    else {
        return Err(format!("missing fixture account for {pointer}"));
    };
    let member = if owner { "owner" } else { "id" };
    account[member] = new.clone();
    if pair {
        for side in ["source", "destination"] {
            if output[side]["id"] == id || side == account_key {
                output[side][member] = new.clone();
            }
            if !owner && input[side] == id {
                input[side] = new.clone();
            }
        }
    } else {
        output[member] = new.clone();
        if !owner {
            input["id"] = new.clone();
        }
    }
    Ok(())
}
pub(super) const BALANCE: &str = "urn:oddlang:ledger-http:balance";

fn rename_balance(value: &mut Value, from: &str, to: &str) {
    if from == to {
        return;
    }
    if let Some(record) = value.as_object_mut() {
        if let Some(balance) = record.remove(from) {
            record.insert(to.to_owned(), balance);
        }
        for nested in record.values_mut() {
            rename_balance(nested, from, to);
        }
    }
}

fn project_scenario(model: &HttpContract, c: &mut Case) -> Result<(), String> {
    let Some(field) = model.fields.get(BALANCE) else {
        return Ok(());
    };
    if field.native_name == field.wire_name {
        return Ok(());
    }
    let responses = c
        .expected
        .iter_mut()
        .filter_map(|e| e.get_mut("response"))
        .chain(
            c.action
                .get_mut("replies")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten(),
        );
    for raw in responses {
        if raw["status"] == 200 {
            let mut value = body(raw)?;
            rename_balance(&mut value, &field.native_name, &field.wire_name);
            put(raw, &value);
        }
    }
    Ok(())
}

fn derive(model: &HttpContract, s: &Slot, sources: &[Case]) -> Result<Case, String> {
    let op = &model.operations[s.operation.as_str()];
    let balance = &model
        .fields
        .get(BALANCE)
        .ok_or("expected ledger balance")?
        .wire_name;
    let get = op.method == "GET";
    let provider = s.role == "http.provider";
    let incoming = if provider { "request" } else { "response" };
    let rule = s.rule.rsplit(':').next().unwrap();
    let kind = s.kind.as_str();
    let src = template(sources, s, if get { "maximum" } else { "success" })?;
    let minimum = if get { 1 } else { 2 };
    if src.fixture.as_array().is_none_or(|a| {
        a.len() < minimum
            || a.iter().any(|v| {
                !v["id"].is_string() || !v["owner"].is_string() || !v["balance"].is_string()
            })
    }) || src.expected[0]["response"]["status"] != 200
    {
        return Err(format!("invalid success template {}", src.case_id));
    }
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
                "dot" => (".", if provider { "%2E" } else { "." }),
                "dot-dot" => ("..", if provider { "%2E%2E" } else { ".." }),
                _ => return Err(format!("unknown route kind {kind}")),
            };
            req["path"] = json!(op.path.replace("{id}", wire));
            input["id"] = json!(id);
            if kind.starts_with("bad-") || (provider && ["empty", "dot", "dot-dot"].contains(&kind))
            {
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
                "unsupported" | "media-before-body" => json!("text/plain"),
                "wrong-charset" => json!("application/json; charset=latin1"),
                "extra-parameter" => json!("application/json; charset=utf-8; extra=one"),
                _ => json!("application/json"),
            };
            if kind == "media-before-body" {
                raw_input = true;
                bytes(&mut req, b"{");
            }
            if matches!(
                kind,
                "missing"
                    | "unsupported"
                    | "media-before-body"
                    | "wrong-charset"
                    | "extra-parameter"
            ) {
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
                .decode(raw["bodyBase64"].as_str().ok_or("expected base64 body")?)
                .map_err(|e| e.to_string())?;
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
                    duplicate_member(&v, key)?
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
                "negative-zero" => Some(json!("-0")),
                "leading-plus" => Some(json!("+1")),
                "exponent" => Some(json!("1e3")),
                "whitespace" => Some(json!(" 1")),
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
                    | "negative-zero"
                    | "leading-plus"
                    | "exponent"
                    | "whitespace"
                    | "overflow"
                    | "numeric"
                    | "below"
                    | "above"
            );
            if let Some(new) = replacement {
                *value.pointer_mut(&pointer).unwrap() = new.clone();
                if !invalid && (provider || pos == "request") {
                    if rule == "text" {
                        set_account_text(
                            &mut c.fixture,
                            &mut input,
                            &mut output,
                            pos,
                            &pointer,
                            &field,
                            &new,
                        )?;
                    } else if !get && pos == "request" && pointer == "/amount" {
                        let n = new
                            .as_str()
                            .ok_or("expected decimal string")?
                            .parse::<i64>()
                            .map_err(|e| e.to_string())?;
                        if n <= 0 {
                            let err = "urn:oddlang:ledger:error:NonPositiveAmount";
                            output = json!({"error":err});
                            reply = response(422, &output);
                            native = "namedError";
                        } else {
                            c.fixture[0]["balance"] = json!(n.to_string());
                            c.fixture[1]["balance"] = json!("0");
                            output["source"][balance] = json!("0");
                            output["destination"][balance] = new;
                        }
                    } else if get {
                        c.fixture[0]["balance"] = new;
                    } else if pos == "response"
                        && pointer.rsplit('/').next() == Some(balance.as_str())
                    {
                        let source = pointer.starts_with("/source/");
                        let n = new
                            .as_str()
                            .ok_or("expected decimal string")?
                            .parse::<i64>()
                            .map_err(|e| e.to_string())?;
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
                            output[other][balance] =
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
                req["path"] = json!(op.path.replace(
                    "{id}",
                    &component(input["id"].as_str().ok_or("expected text ID")?)
                ))
            }
            if get && provider && pos == "response" {
                req["path"] = json!(op.path.replace(
                    "{id}",
                    &component(c.fixture[0]["id"].as_str().ok_or("expected fixture ID")?)
                ))
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
                let mut acct = if reply["status"] == 200 {
                    ["source", "destination"]
                        .iter()
                        .find_map(|k| {
                            (output[*k]["id"] == initial["id"]).then(|| output[*k].clone())
                        })
                        .unwrap_or_else(|| initial.clone())
                } else {
                    initial.clone()
                };
                rename_balance(&mut acct, "balance", balance);
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
        rename_balance(&mut output, balance, "balance");
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
        let authored: Vec<Case> = serde_json::from_slice(
            model
                .source_files
                .get(file)
                .ok_or_else(|| format!("missing scenario file {file}"))?,
        )
        .map_err(|e| format!("{file}: {e}"))?;
        for mut c in authored {
            project_scenario(model, &mut c)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_key_escapes_supplementary_and_quoted_characters() {
        let record = json!({"🪶é\"":7});
        let bytes = duplicate_member(&record, "🪶é\"").unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), record);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains(r"\ud83e\udeb6\u00e9\u0022")
        );
    }
}
