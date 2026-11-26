use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sensor {
    pub channel: u16,
    pub meter: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Irrigation {
    Idle,
    Sample(Sensor),
    AdjustRate {
        enabled: bool,
        flow: u32,
        tags: Vec<u8>,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    #[serde(with = "signature_text")]
    pub signature: [u8; 2],
    #[serde(with = "chunks_text")]
    pub chunks: Vec<Vec<u8>>,
}

mod signature_text {
    use serde::{de::Error as _, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &[u8; 2], writer: S) -> Result<S::Ok, S::Error> {
        writer.serialize_str(&format!("{:04x}", u16::from_be_bytes(*value)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(reader: D) -> Result<[u8; 2], D::Error> {
        let text = String::deserialize(reader)?;
        let value = u16::from_str_radix(&text, 16).map_err(D::Error::custom)?;
        if format!("{value:04x}") != text {
            return Err(D::Error::custom("signature text"));
        }
        Ok(value.to_be_bytes())
    }
}

mod chunks_text {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(value: &[Vec<u8>], writer: S) -> Result<S::Ok, S::Error> {
        let text = value
            .iter()
            .map(|part| STANDARD.encode(part))
            .collect::<Vec<_>>();
        text.serialize(writer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(reader: D) -> Result<Vec<Vec<u8>>, D::Error> {
        let text = Vec::<String>::deserialize(reader)?;
        let list_error = || D::Error::custom("piece list limit");
        if text.len() > 2 {
            return Err(list_error());
        }
        let mut result = Vec::new();
        for item in text {
            if item.len() > 8 {
                return Err(D::Error::custom("piece text limit"));
            }
            let bytes = STANDARD
                .decode(item)
                .map_err(|_| D::Error::custom("piece encoding"))?;
            if bytes.len() > 4 {
                return Err(D::Error::custom("piece byte limit"));
            }
            result.push(bytes);
            if result.iter().map(|part| part.len() + 1).sum::<usize>() > 6 {
                return Err(list_error());
            }
        }
        Ok(result)
    }
}
