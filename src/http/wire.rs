//! Compare observed wire bytes structurally only when both bodies are strict JSON.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, json};
use std::fmt;
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("strict JSON")
            }
            fn visit_unit<E>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_bool<E>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(json!(v)))
            }
            fn visit_str<E>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(json!(v)))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(json!(v)))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(json!(v)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Strict, E> {
                Ok(Strict(json!(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = vec![];
                while let Some(Strict(x)) = a.next_element()? {
                    v.push(x)
                }
                Ok(Strict(json!(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some((k, Strict(x))) = a.next_entry::<String, Strict>()? {
                    if v.insert(k, x).is_some() {
                        return Err(de::Error::custom("duplicate member"));
                    }
                }
                Ok(Strict(Value::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}
fn segment(s: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        out.push(if b == b'%' {
            let h = (bytes.next()? as char).to_digit(16)?;
            let l = (bytes.next()? as char).to_digit(16)?;
            (h * 16 + l) as u8
        } else {
            b
        });
    }
    String::from_utf8(out).ok()
}
fn normalize(ob: &Value) -> Value {
    let mut ob = ob.clone();
    for key in ["request", "response"] {
        let Some(raw) = ob.get_mut(key).and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(bytes) = raw
            .get("bodyBase64")
            .and_then(Value::as_str)
            .and_then(|s| STANDARD.decode(s).ok())
            && let Ok(Strict(v)) = serde_json::from_slice(&bytes)
        {
            raw.insert("bodyBase64".into(), json!([v]));
        }
        if let Some(path) = raw.get("path").and_then(Value::as_str)
            && let Some(segments) = path.split('/').map(segment).collect::<Option<Vec<_>>>()
        {
            raw.insert("path".into(), json!(segments));
        }
        if let Some(ct) = raw.get("contentType").and_then(Value::as_str) {
            let value = ct.to_ascii_lowercase();
            let params = value.split(';').map(str::trim).collect::<Vec<_>>();
            let json_media = match params.as_slice() {
                ["application/json"] => true,
                ["application/json", charset] => {
                    charset.split_once('=').is_some_and(|(key, value)| {
                        key.trim() == "charset" && matches!(value.trim(), "utf-8" | "\"utf-8\"")
                    })
                }
                _ => false,
            };
            let ct = if json_media {
                "application/json".into()
            } else {
                value
            };
            raw.insert("contentType".into(), json!(ct));
        }
    }
    ob
}
fn envelope(v: &Value, request: bool) -> bool {
    let Some(raw) = v.as_object() else {
        return false;
    };
    let common = raw
        .get("bodyBase64")
        .and_then(Value::as_str)
        .is_some_and(|s| STANDARD.decode(s).is_ok())
        && raw
            .get("contentType")
            .is_some_and(|ct| ct.is_null() || ct.is_string());
    common
        && if request {
            raw.len() == 4 && v["method"].is_string() && v["path"].is_string()
        } else {
            raw.len() == 3
                && v["status"]
                    .as_u64()
                    .is_some_and(|s| (100..600).contains(&s))
        }
}
fn valid(ob: &Value) -> bool {
    let Some(fields) = ob.as_object() else {
        return false;
    };
    if let Some(native) = fields.get("native") {
        return fields.len() == 1
            && native
                .as_object()
                .is_some_and(|r| match native["kind"].as_str() {
                    Some("success") => r.len() == 2 && r.contains_key("value"),
                    Some("namedError") => r.len() == 2 && native["error"].is_string(),
                    Some("protocolError" | "localRejection") => r.len() == 1,
                    _ => false,
                });
    }
    fields.len() == 2
        && envelope(&ob["request"], true)
        && fields
            .get("response")
            .is_some_and(|r| r.is_null() || envelope(r, false))
}
pub(super) fn valid_case(c: &super::Case) -> bool {
    if c.case_id.is_empty()
        || !c.fixture.is_array()
        || c.expected.is_empty()
        || !c.expected.iter().all(valid)
    {
        return false;
    }
    if c.role == "json.codec" {
        return c.expected.len() == 1
            && c.fixture == json!([])
            && c.action.as_object().is_some_and(|a| a.len() == 1)
            && c.action["bodyBase64"]
                .as_str()
                .is_some_and(|s| STANDARD.decode(s).is_ok())
            && matches!(
                c.expected[0]["native"]["kind"].as_str(),
                Some("success" | "localRejection")
            );
    }
    if c.role != "http.provider" && c.role != "http.consumer" {
        return false;
    }
    let exchanges: Vec<_> = c
        .expected
        .iter()
        .take_while(|v| v.get("request").is_some())
        .collect();
    if c.role == "http.provider" {
        return exchanges.len() == c.expected.len()
            && c.action
                == json!({"requests":exchanges.iter().map(|v|&v["request"]).collect::<Vec<_>>()})
            && exchanges.iter().all(|v| !v["response"].is_null());
    }
    let native = &c.expected.last().unwrap()["native"];
    exchanges.len() + 1 == c.expected.len()
        && native.is_object()
        && (native["kind"] == "localRejection") == exchanges.is_empty()
        && c.action["input"].is_object()
        && c.action
            == json!({"input":c.action["input"],"replies":exchanges.iter().map(|v|&v["response"]).collect::<Vec<_>>()})
}
pub fn compare_observations(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| valid(y) && normalize(x) == normalize(y))
}
