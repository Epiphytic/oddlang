use ::base64::{Engine as _, engine::general_purpose::STANDARD};
use ::serde::{Deserialize, Deserializer, Serializer, de::Error as _};
pub fn serialize<S: Serializer>(bytes: &[u8], writer: S) -> Result<S::Ok, S::Error> {
    writer.serialize_str(&STANDARD.encode(bytes))
}
pub fn deserialize<'de, D: Deserializer<'de>>(reader: D) -> Result<Vec<u8>, D::Error> {
    let encoded = String::deserialize(reader)?;
    if encoded.len() > (MAX.div_ceil(3) * 4) as usize {
        return Err(D::Error::custom(ENCODED));
    }
    let bytes = STANDARD
        .decode(&encoded)
        .map_err(|_| D::Error::custom(SYNTAX))?;
    if bytes.len() > MAX as usize {
        return Err(D::Error::custom(DECODED));
    }
    if STANDARD.encode(&bytes) != encoded {
        return Err(D::Error::custom(SYNTAX));
    }
    Ok(bytes)
}
