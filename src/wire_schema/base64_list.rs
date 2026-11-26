mod __oddlang_wire_base64_list {
    use ::base64::{Engine as _, engine::general_purpose::STANDARD};
    use ::serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

    pub struct Policy {
        pub items: u32,
        pub item_bytes: u32,
        pub total_bytes: u32,
        pub overhead: u32,
        pub list: &'static str,
        pub encoded: &'static str,
        pub decoded: &'static str,
        pub syntax: &'static str,
    }

    pub fn serialize<S: Serializer>(items: &[Vec<u8>], writer: S) -> Result<S::Ok, S::Error> {
        items
            .iter()
            .map(|bytes| STANDARD.encode(bytes))
            .collect::<Vec<_>>()
            .serialize(writer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        reader: D,
        policy: Policy,
    ) -> Result<Vec<Vec<u8>>, D::Error> {
        let text = Vec::<String>::deserialize(reader)?;
        if text.len() > policy.items as usize {
            return Err(D::Error::custom(policy.list));
        }
        let item_limit = policy.item_bytes as usize;
        let encoded_limit = item_limit.div_ceil(3) * 4;
        let mut total = 0u64;
        text.into_iter()
            .map(|value| {
                if value.len() > encoded_limit {
                    return Err(D::Error::custom(policy.encoded));
                }
                let bytes = STANDARD
                    .decode(&value)
                    .map_err(|_| D::Error::custom(policy.syntax))?;
                if bytes.len() > item_limit {
                    return Err(D::Error::custom(policy.decoded));
                }
                if STANDARD.encode(&bytes) != value {
                    return Err(D::Error::custom(policy.syntax));
                }
                total = total
                    .checked_add(bytes.len() as u64 + u64::from(policy.overhead))
                    .ok_or_else(|| D::Error::custom(policy.list))?;
                if total > u64::from(policy.total_bytes) {
                    return Err(D::Error::custom(policy.list));
                }
                Ok(bytes)
            })
            .collect()
    }
}
