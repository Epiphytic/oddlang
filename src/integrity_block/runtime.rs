use ::sha2::{Digest, Sha256};

pub(super) enum Fault {
    Size,
    Offset,
    Representation,
    PayloadSyntax,
    PayloadDecodedSize,
}

pub(super) enum Check {
    RawSize,
    EncodedSize,
    Offset,
    RepresentationSize,
}

pub(super) struct Frame<'a> {
    pub offset: u64,
    pub size: u32,
    pub compressed: bool,
    pub digest: &'a [u8; 32],
    pub payload: &'a [u8],
}

pub(super) fn checksum(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(super) fn end(offset: u64, size: u32) -> Result<u64, Fault> {
    match offset.checked_add(u64::from(size)) {
        Some(end) if end <= OFFSET_MAX => Ok(end),
        _ => Err(Fault::Offset),
    }
}

pub(super) fn raw_size(size: usize) -> Result<(), Fault> {
    if (RAW_MIN..=RAW_MAX).contains(&size) {
        Ok(())
    } else {
        Err(Fault::Size)
    }
}

pub(super) fn encoded_size(size: usize) -> bool {
    (ENCODED_MIN..=ENCODED_MAX).contains(&size)
}

pub(super) fn validate(frame: &Frame<'_>) -> Result<(), Fault> {
    for check in CHECKS {
        match check {
            Check::RawSize => raw_size(frame.size as usize)?,
            Check::EncodedSize if !encoded_size(frame.payload.len()) => return Err(Fault::Size),
            Check::Offset => {
                end(frame.offset, frame.size)?;
            }
            Check::RepresentationSize => {
                let represented = frame.payload.len();
                let declared = frame.size as usize;
                let valid = if frame.compressed {
                    represented < declared
                } else {
                    represented == declared
                };
                if !valid {
                    return Err(Fault::Representation);
                }
            }
            Check::EncodedSize => {}
        }
    }
    Ok(())
}

pub(super) fn verify_checksum(frame: &Frame<'_>, raw: &[u8]) -> Result<(), Fault> {
    if &checksum(raw) == frame.digest {
        Ok(())
    } else {
        Err(Fault::Representation)
    }
}

fn lower_hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|&byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

pub(super) mod hex {
    pub fn serialize<S: ::serde::Serializer>(
        bytes: &[u8; 32],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::lower_hex(bytes))
    }

    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; 32], D::Error> {
        use ::serde::{Deserialize, de::Error as _};

        fn digit(byte: u8) -> Option<u8> {
            let n = b"0123456789abcdef"
                .iter()
                .position(|&candidate| candidate == byte)?;
            Some(n as u8)
        }

        let text = String::deserialize(deserializer)?;
        let mut output = [0u8; 32];
        if text.len() != 2 * output.len() {
            return Err(D::Error::custom(super::HEX_SYNTAX));
        }
        for (pair, dest) in text.as_bytes().chunks_exact(2).zip(&mut output) {
            let high = digit(pair[0]).ok_or_else(|| D::Error::custom(super::HEX_SYNTAX))?;
            let low = digit(pair[1]).ok_or_else(|| D::Error::custom(super::HEX_SYNTAX))?;
            *dest = (high << 4) | low;
        }
        Ok(output)
    }
}

pub(super) fn decode_payload(text: &str, limit: usize) -> Result<Vec<u8>, Fault> {
    use ::base64::{Engine as _, engine::general_purpose::STANDARD};
    if text.len() > limit.div_ceil(3).saturating_mul(4) {
        return Err(Fault::Size);
    }
    let bytes = STANDARD.decode(text).map_err(|_| Fault::PayloadSyntax)?;
    if bytes.len() > limit {
        return Err(Fault::PayloadDecodedSize);
    }
    if STANDARD.encode(&bytes) != text {
        return Err(Fault::PayloadSyntax);
    }
    Ok(bytes)
}
