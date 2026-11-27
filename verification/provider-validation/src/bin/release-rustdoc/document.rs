use super::*;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use std::fmt;

#[derive(Deserialize)]
pub struct Document {
    pub root: Value,
    pub target: Value,
    pub format_version: u64,
    pub includes_private: bool,
    #[serde(deserialize_with = "items")]
    pub index: Vec<Value>,
}

// Multiple items can share a lexical key. Preserve each witness in the
// original Rustdoc index order, including ambiguous keys in old receipts.
fn items<'de, D: Deserializer<'de>>(source: D) -> std::result::Result<Vec<Value>, D::Error> {
    struct Items;
    impl<'de> Visitor<'de> for Items {
        type Value = Vec<Value>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a Rustdoc item index")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut rows = Vec::with_capacity(map.size_hint().unwrap_or(0));
            while let Some((_, row)) = map.next_entry::<String, Value>()? {
                rows.push(row);
            }
            Ok(rows)
        }
    }
    source.deserialize_map(Items)
}
impl Document {
    pub fn read(path: &Path, triple: &str) -> Result<Self> {
        let doc: Self = serde_json::from_slice(&fs::read(path)?)?;
        if doc.format_version != 57 || !doc.includes_private {
            return Err("unsupported rustdoc schema/scope".into());
        }
        if doc.target["triple"] != triple {
            return Err("wrong documented target".into());
        }
        Ok(doc)
    }
    pub fn into_index(self) -> Value {
        let index: serde_json::Map<_, _> = self
            .index
            .into_iter()
            .map(|r| (r["id"].to_string(), r))
            .collect();
        json!({"index":index})
    }
}
