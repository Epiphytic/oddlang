wit_bindgen::generate!({ path: "wit", world: "framing" });

use exports::oddlang::framing::codec::{FrameError, Guest};
use marmish_core::ProbeEnvelope;

struct Codec;

fn error(e: marmish_core::FrameError) -> FrameError {
    match e {
        marmish_core::FrameError::SizeLimit => FrameError::SizeLimit,
        marmish_core::FrameError::InvalidJson => FrameError::InvalidJson,
        marmish_core::FrameError::InvalidVersion => FrameError::InvalidVersion,
        marmish_core::FrameError::InvalidEncoding => FrameError::InvalidEncoding,
    }
}

impl Guest for Codec {
    fn encode(input: Vec<u8>) -> Result<Vec<u8>, FrameError> {
        ProbeEnvelope::encode(&input).map_err(error)
    }
    fn decode(input: Vec<u8>) -> Result<Vec<u8>, FrameError> {
        ProbeEnvelope::decode(&input).map_err(error)
    }
}

export!(Codec);
