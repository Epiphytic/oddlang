use super::ordered_fields;
use crate::model::Model;
use serde_json::json;

pub fn generate(model: &Model) -> String {
    let fields: Vec<_> = ordered_fields(model)
        .into_iter()
        .map(|f| {
            let ty = if f.range.is_some() { "long" } else { "string" };
            let mut field = json!({"name": f.avro_name, "type": ty});
            if !f.aliases.is_empty() {
                field["aliases"] = json!(f.aliases);
            }
            field
        })
        .collect();
    let schema = json!({"name": model.avro_name, "type": "record", "fields": fields});
    format!("{schema:#}\n")
}
