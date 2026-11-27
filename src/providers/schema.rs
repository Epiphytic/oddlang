//! Closed, bounded vocabulary for workspace-provided document schemas.
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Schema {
    #[serde(rename = "type")]
    kind: Kind,
    #[serde(default, deserialize_with = "supplied")]
    properties: Option<BTreeMap<String, Schema>>,
    #[serde(default, deserialize_with = "supplied")]
    required: Option<Vec<String>>,
    #[serde(default, deserialize_with = "supplied")]
    additional_properties: Option<bool>,
    #[serde(default, deserialize_with = "supplied")]
    items: Option<Box<Schema>>,
    #[serde(default, deserialize_with = "supplied")]
    format: Option<StringFormat>,
    #[serde(default, deserialize_with = "supplied")]
    iri_namespaces: Option<Vec<String>>,
}
// Missing keywords use Option::default. Present keywords must deserialize as their declared type.
fn supplied<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum StringFormat {
    Iri,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Object,
    Array,
    String,
    Integer,
    Number,
    Boolean,
    Null,
}

impl Schema {
    pub(super) fn check(&self, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("provider schema nesting limit".into());
        }
        if self.kind != Kind::Object
            && (self.properties.is_some()
                || self.required.is_some()
                || self.additional_properties.is_some())
        {
            return Err("object schema keyword on another type".into());
        }
        if self.kind != Kind::Array && self.items.is_some() {
            return Err("items requires an array schema".into());
        }
        if self.format.is_some() && self.kind != Kind::String {
            return Err("iri format requires a string schema".into());
        }
        if let Some(namespaces) = &self.iri_namespaces {
            let mut seen = BTreeSet::new();
            if self.format.is_none()
                || namespaces.is_empty()
                || namespaces.len() > 64
                || namespaces.iter().any(|ns| {
                    !ns.ends_with([':', '/', '#'])
                        || oxigraph::model::NamedNode::new(ns.as_str()).is_err()
                        || !seen.insert(ns)
                })
            {
                return Err(
                    "iriNamespaces requires distinct absolute IRI namespaces and iri format".into(),
                );
            }
        }
        let mut keys = BTreeSet::new();
        for key in self.required.iter().flatten() {
            if !keys.insert(key)
                || !self
                    .properties
                    .as_ref()
                    .is_some_and(|p| p.contains_key(key))
            {
                return Err(format!("duplicate or undeclared required property {key}"));
            }
        }
        for child in self.properties.iter().flat_map(|p| p.values()) {
            child.check(depth + 1)?;
        }
        if let Some(child) = &self.items {
            child.check(depth + 1)?;
        }
        Ok(())
    }

    pub(super) fn validate(&self, value: &Value) -> Result<(), String> {
        let matches = match self.kind {
            Kind::Object => value.is_object(),
            Kind::Array => value.is_array(),
            Kind::String => value.is_string(),
            Kind::Integer => value.is_u64() || value.is_i64(),
            Kind::Number => value.is_number(),
            Kind::Boolean => value.is_boolean(),
            Kind::Null => value.is_null(),
        };
        if !matches {
            return Err("provider input has the wrong schema type".into());
        }
        if self.format.is_some() {
            let id = value.as_str().ok_or("expected entity IRI")?;
            oxigraph::model::NamedNode::new(id).map_err(|_| "expected absolute entity IRI")?;
            if let Some(namespaces) = &self.iri_namespaces
                && !namespaces
                    .iter()
                    .any(|ns| id.strip_prefix(ns).is_some_and(|local| !local.is_empty()))
            {
                return Err("entity IRI outside declared namespaces".into());
            }
        }
        if let Some(map) = value.as_object() {
            for key in self.required.iter().flatten() {
                if !map.contains_key(key) {
                    return Err(format!("missing provider property {key}"));
                }
            }
            for (key, item) in map {
                if let Some(property) = self.properties.as_ref().and_then(|p| p.get(key)) {
                    property.validate(item).map_err(|e| format!("{key}: {e}"))?;
                } else if self.additional_properties == Some(false) {
                    return Err(format!("unexpected provider property {key}"));
                }
            }
        }
        if let (Some(items), Some(values)) = (&self.items, value.as_array()) {
            for item in values {
                items.validate(item)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entity_ranges_apply_to_every_list_item_and_keep_prose_free() {
        let schema: Schema=serde_json::from_value(json!({"type":"object","properties":{
            "label":{"type":"string"},
            "uses":{"type":"array","items":{"type":"string","format":"iri","iriNamespaces":["urn:oddlang:work:"]}}
        }})).unwrap();
        schema.check(0).unwrap();
        let value = |id| json!({"label":"release task","uses":["urn:oddlang:work:plan",id]});
        schema.validate(&value("urn:oddlang:work:design")).unwrap();
        for bad in [
            "release task",
            "_:blank",
            "urn:oddlang:work:",
            "urn:oddlang:works:plan",
            "urn:oddlang:work:bad id",
        ] {
            assert!(schema.validate(&value(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn supplied_schema_keywords_must_have_their_declared_types() {
        for (kind, keyword) in [
            ("object", "properties"),
            ("object", "required"),
            ("object", "additionalProperties"),
            ("array", "items"),
            ("string", "format"),
            ("string", "iriNamespaces"),
        ] {
            let mut declaration = json!({"type": kind});
            if keyword == "iriNamespaces" {
                declaration["format"] = json!("iri");
            }
            serde_json::from_value::<Schema>(declaration.clone())
                .unwrap()
                .check(0)
                .unwrap();
            declaration[keyword] = Value::Null;
            assert!(
                serde_json::from_value::<Schema>(declaration).is_err(),
                "{keyword}"
            );
        }
        // Null remains a valid input value when the provider explicitly declares that type.
        let nullable: Schema = serde_json::from_value(json!({"type": "null"})).unwrap();
        nullable.validate(&Value::Null).unwrap();
    }

    #[test]
    fn reject_ambiguous_namespace_boundaries_and_mistyped_schema() {
        for raw in [
            json!({"type":"string","iriNamespaces":["urn:x:"]}),
            json!({"type":"string","format":"iri","iriNamespaces":["urn:x"]}),
            json!({"type":"string","format":"iri","iriNamespaces":["no space:"]}),
            json!({"type":"object","format":"iri"}),
        ] {
            assert!(
                serde_json::from_value::<Schema>(raw)
                    .unwrap()
                    .check(0)
                    .is_err()
            );
        }
    }
}
