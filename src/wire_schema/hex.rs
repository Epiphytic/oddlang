mod __oddlang_wire_hex {
    pub fn serialize<S: ::serde::Serializer, const N: usize>(
        bytes: &[u8; N],
        writer: S,
    ) -> Result<S::Ok, S::Error> {
        let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        writer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: ::serde::Deserializer<'de>, const N: usize>(
        reader: D,
        diagnostic: &str,
    ) -> Result<[u8; N], D::Error> {
        use ::serde::{Deserialize, de::Error as _};
        let text = String::deserialize(reader)?;
        let mut bytes = [0; N];
        if text.len() != N * 2 {
            return Err(D::Error::custom(diagnostic));
        }
        for (chunk, dest) in text.as_bytes().chunks_exact(2).zip(&mut bytes) {
            let decode = |byte| b"0123456789abcdef".iter().position(|&digit| digit == byte);
            let high = decode(chunk[0]).ok_or_else(|| D::Error::custom(diagnostic))? as u8;
            let low = decode(chunk[1]).ok_or_else(|| D::Error::custom(diagnostic))? as u8;
            *dest = high << 4 | low;
        }
        Ok(bytes)
    }
}
