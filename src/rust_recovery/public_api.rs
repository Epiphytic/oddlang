//! Independent witness for the native public surface of a crate exporting concrete value types.
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

fn list(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| "expected rustdoc array".into())
}

fn name(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(|| "expected rustdoc name".into())
}

fn id(v: &Value) -> Result<String> {
    v.as_u64()
        .map(|n| n.to_string())
        .ok_or_else(|| "missing rustdoc identity".into())
}

struct Surface {
    doc: Value,
    exports: BTreeMap<String, String>,
}

impl Surface {
    fn item(&self, id: &Value) -> Result<&Value> {
        self.doc["index"]
            .get(self::id(id)?)
            .ok_or_else(|| "unresolved local item".into())
    }

    fn path(&self, node: &Value) -> Result<String> {
        let reference = id(&node["id"])?;
        if let Some(public) = self.exports.get(&reference) {
            return Ok(format!("local::{public}"));
        }
        let path = &self.doc["paths"][&reference];
        if path["crate_id"].as_u64().is_none_or(|n| n == 0) {
            return Err(format!("unresolved public path for {reference}"));
        }
        Ok(list(&path["path"])?
            .iter()
            .map(name)
            .collect::<Result<Vec<_>>>()?
            .join("::"))
    }

    fn ty(&self, ty: &Value) -> Result<Value> {
        if let Some(p) = ty.get("primitive") {
            return Ok(json!({"primitive":name(p)?}));
        }
        if ty["generic"] == "Self" {
            return Ok(json!({"receiverType":true}));
        }
        if let Some(s) = ty.get("slice") {
            return Ok(json!({"sequence":self.ty(s)?}));
        }
        if let Some(r) = ty.get("borrowed_ref") {
            return Ok(
                json!({"borrows":self.ty(&r["type"])?,"mutable":r["is_mutable"].as_bool().ok_or("missing mutability")?,"lifetime":r["lifetime"]}),
            );
        }
        if let Some(p) = ty.get("resolved_path") {
            let mut args = Vec::new();
            if !p["args"].is_null() {
                let a = &p["args"]["angle_bracketed"];
                if !list(&a["constraints"])?.is_empty() {
                    return Err("unsupported associated constraint".into());
                }
                for arg in list(&a["args"])? {
                    args.push(self.ty(&arg["type"])?);
                }
            }
            return Ok(json!({"named":self.path(p)?,"arguments":args}));
        }
        Err(format!(
            "unsupported public type shape {:?}",
            ty.as_object().map(|o| o.keys().collect::<Vec<_>>())
        ))
    }

    fn member(&self, inner: &Value) -> Result<Value> {
        if let Some(c) = inner.get("assoc_const") {
            return Ok(json!({"constantType":self.ty(&c["type"])?}));
        }
        let f = inner.get("function").ok_or("unsupported associated item")?;
        concrete(&f["generics"])?;
        let sig = &f["sig"];
        let mut params = Vec::new();
        for pair in list(&sig["inputs"])? {
            params.push(json!({"receiver":pair[0]=="self","type":self.ty(&pair[1])?}));
        }
        let ret = if sig["output"].is_null() {
            Value::Null
        } else {
            self.ty(&sig["output"])?
        };
        Ok(
            json!({"parameters":params,"result":ret,"header":f["header"],"variadic":sig["is_c_variadic"]}),
        )
    }

    fn declaration(&self, item: &Value) -> Result<Value> {
        let (shape, data) = if let Some(data) = item["inner"].get("enum") {
            if data["has_stripped_variants"] != false {
                return Err("enum has hidden variants".into());
            }
            let mut variants = Vec::new();
            for key in list(&data["variants"])? {
                let v = self.item(key)?;
                let member = &v["inner"]["variant"];
                if member["kind"] != "plain" {
                    return Err("only unit variants supported".into());
                }
                variants.push(json!({"name":name(&v["name"])?,"explicitValue":member["discriminant"]["value"]}));
            }
            (json!({"variants":variants}), data)
        } else {
            let data = item["inner"]
                .get("struct")
                .ok_or("expected struct or enum export")?;
            let shape = if data["kind"] == "unit" {
                json!("unit")
            } else if let Some(fields) = data["kind"]["tuple"].as_array() {
                if fields.iter().any(|f| !f.is_null()) {
                    return Err("public tuple field is outside this witness".into());
                }
                json!({"privateTupleArity":fields.len()})
            } else {
                return Err("unsupported struct field shape".into());
            };
            (shape, data)
        };
        concrete(&data["generics"])?;
        let mut traits = Vec::new();
        let mut members = Map::new();
        for key in list(&data["impls"])? {
            let implementation = &self.item(key)?["inner"]["impl"];
            if !implementation["blanket_impl"].is_null() {
                continue;
            }
            concrete(&implementation["generics"])?;
            if !implementation["trait"].is_null() {
                let t = &implementation["trait"];
                if !t["args"].is_null() {
                    return Err("parameterized trait outside witness".into());
                }
                traits.push(json!({"trait":self.path(t)?,"negative":implementation["is_negative"],"unsafe":implementation["is_unsafe"]}));
            } else {
                for key in list(&implementation["items"])? {
                    let m = self.item(key)?;
                    if m["visibility"] == "public"
                        && members
                            .insert(name(&m["name"])?.into(), self.member(&m["inner"])?)
                            .is_some()
                    {
                        return Err("duplicate inherent member".into());
                    }
                }
            }
        }
        traits.sort_by_cached_key(Value::to_string);
        Ok(json!({"shape":shape,"members":members,"traits":traits}))
    }
}

fn concrete(generics: &Value) -> Result<()> {
    if generics != &json!({"params":[],"where_predicates":[]}) {
        return Err("generic declarations outside witness".into());
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<Value> {
    let doc: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if doc["format_version"] != 57 || doc["includes_private"] != false {
        return Err("expected public rustdoc format 57".into());
    }
    let mut surface = Surface {
        doc,
        exports: BTreeMap::new(),
    };
    let root = surface.item(&surface.doc["root"])?;
    let crate_name = name(&root["name"])?.to_owned();
    let mut exports = BTreeMap::new();
    for key in list(&root["inner"]["module"]["items"])? {
        let i = surface.item(key)?;
        let (alias, target) = if let Some(u) = i["inner"].get("use") {
            if u["is_glob"] != false {
                return Err("glob exports outside witness".into());
            }
            (name(&u["name"])?, id(&u["id"])?)
        } else {
            (name(&i["name"])?, id(key)?)
        };
        if exports.insert(target, alias.to_owned()).is_some() {
            return Err("multiple aliases outside witness".into());
        }
    }
    surface.exports = exports;
    let mut types = Map::new();
    for (key, alias) in &surface.exports {
        if types
            .insert(
                alias.to_owned(),
                surface.declaration(&surface.doc["index"][key])?,
            )
            .is_some()
        {
            return Err("duplicate public name".into());
        }
    }
    Ok(
        json!({"format":"oddlang-native-value-api-v1","crate":crate_name,"target":surface.doc["target"]["triple"],"types":types}),
    )
}
