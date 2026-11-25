use super::*;
use serde_json::{Map, Value, json};
fn ref_name(i: &Iri) -> String {
    i.to_string().replace('~', "~0").replace('/', "~1")
}
fn reference(i: &Iri) -> Value {
    json!({"$ref":format!("#/$defs/{}",ref_name(i))})
}
pub(super) fn schema(c: &HttpContract) -> Value {
    let mut defs = Map::new();
    for (id, r) in &c.records {
        let mut props = Map::new();
        for fid in &r.fields {
            let f = &c.fields[fid];
            let mut shape = match f.shape.as_str() {
                "urn:oddlang:http:Text" => json!({"type":"string"}),
                "urn:oddlang:http:Boolean" => json!({"type":"boolean"}),
                "urn:oddlang:http:I64" => {
                    json!({"type":"string","pattern":"^(0|-?[1-9][0-9]*)$","x-decimal-range":[i64::MIN.to_string(),i64::MAX.to_string()]})
                }
                _ => reference(&f.shape),
            };
            for cid in &f.constraints {
                if let Constraint::Bounds { min, max } = c.constraints[cid] {
                    shape["x-decimal-range"] = json!([min.to_string(), max.to_string()]);
                }
            }
            props.insert(f.wire_name.clone(), shape);
        }
        let required = props.keys().cloned().collect::<Vec<_>>();
        defs.insert(id.to_string(),json!({"type":"object","additionalProperties":false,"properties":props,"required":required}));
    }
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":defs})
}
pub(super) fn openapi(c: &HttpContract) -> Value {
    let mut paths = json!({});
    let key = |s: &str| {
        format!(
            "Record{}",
            s.bytes().map(|b| format!("{b:02x}")).collect::<String>()
        )
    };
    let reference = |i: &Iri| json!({"$ref":format!("#/components/schemas/{}",key(i.as_str()))});
    let mut schemas = Map::new();
    for (iri, mut record) in schema(c)["$defs"].as_object().unwrap().clone() {
        for prop in record["properties"].as_object_mut().unwrap().values_mut() {
            if let Some(target) = prop["$ref"].as_str() {
                let iri = target
                    .trim_start_matches("#/$defs/")
                    .replace("~1", "/")
                    .replace("~0", "~");
                *prop = json!({"$ref":format!("#/components/schemas/{}",key(&iri))});
            }
        }
        schemas.insert(key(&iri), record);
    }
    for (id, op) in &c.operations {
        let mut operation = json!({"operationId":id.as_str(),"responses":{op.status.to_string():{"description":"Success","content":{"application/json":{"schema":reference(&op.response)}}}}});
        let mut errors = BTreeMap::<u16, Vec<&str>>::new();
        for e in &op.errors {
            errors.entry(c.errors[e]).or_default().push(e.as_str());
        }
        for (status, names) in errors {
            operation["responses"][status.to_string()] = json!({"description":"Named operation error","content":{"application/json":{"schema":{"type":"object","properties":{"error":{"enum":names}},"required":["error"],"additionalProperties":false}}}});
        }
        if op.method == "GET" {
            operation["parameters"] =
                json!([{"name":"id","in":"path","required":true,"schema":{"type":"string"}}]);
        } else {
            operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":reference(&op.request)}}});
        }
        paths[&op.path][op.method.to_lowercase()] = operation;
    }
    json!({"openapi":"3.1.0","info":{"title":c.identity.as_str(),"version":c.version},"paths":paths,"components":{"schemas":schemas}})
}
