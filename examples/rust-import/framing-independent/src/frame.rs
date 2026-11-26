// A small archival record with a seven-byte payload and a distinct version.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits;
impl Limits {
    pub const RAW_BYTES: usize = 7;
    pub const NDJSON_BYTES: usize = 256;
    pub const JSON_BYTES: usize = 128;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("archive exceeds its bound")]
    SizeLimit,
    #[error("unreadable archive record")]
    InvalidJson,
    #[error("archive version mismatch")]
    InvalidVersion,
    #[error("invalid archive content")]
    InvalidEncoding,
}

pub struct ProbeEnvelope;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    payload: String,
}

impl ProbeEnvelope {
    pub fn encode(content: &[u8]) -> Result<Vec<u8>, FrameError> {
        if content.len() > Limits::RAW_BYTES {
            return Err(FrameError::SizeLimit);
        }
        let record = serde_json::to_vec(&Envelope {
            version: 7,
            payload: STANDARD.encode(content),
        })
        .map_err(|_| FrameError::InvalidJson)?;
        if record.len() > Limits::JSON_BYTES {
            return Err(FrameError::SizeLimit);
        }
        Ok(record)
    }

    pub fn decode(input: &[u8]) -> Result<Vec<u8>, FrameError> {
        if input.len() > Limits::JSON_BYTES {
            return Err(FrameError::SizeLimit);
        }
        let decoded: Envelope =
            serde_json::from_slice(input).map_err(|_| FrameError::InvalidJson)?;
        let ceiling = Limits::RAW_BYTES.div_ceil(3) * 4;
        if decoded.payload.len() > ceiling {
            return Err(FrameError::SizeLimit);
        }
        if decoded.version != 7 {
            return Err(FrameError::InvalidVersion);
        }
        let content = STANDARD
            .decode(&decoded.payload)
            .map_err(|_| FrameError::InvalidEncoding)?;
        if content.len() > Limits::RAW_BYTES {
            return Err(FrameError::SizeLimit);
        }
        if STANDARD.encode(&content) != decoded.payload {
            return Err(FrameError::InvalidEncoding);
        }
        Ok(content)
    }
}
