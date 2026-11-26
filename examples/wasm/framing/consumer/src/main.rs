use marmish_core::{FrameError, Limits, ProbeEnvelope};

#[path = "../../batch.rs"]
mod batch;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        (Limits::RAW_BYTES, Limits::JSON_BYTES, Limits::NDJSON_BYTES),
        (16_384, 32_768, 65_536)
    );
    for (error, display) in [
        (FrameError::SizeLimit, "probe frame exceeds its size limit"),
        (FrameError::InvalidJson, "invalid probe JSON"),
        (FrameError::InvalidVersion, "unsupported probe version"),
        (
            FrameError::InvalidEncoding,
            "invalid canonical base64 payload",
        ),
    ] {
        assert_eq!(error.to_string(), display);
    }
    batch::run(|encode, input| {
        let call: fn(&[u8]) -> Result<Vec<u8>, FrameError> = if encode {
            ProbeEnvelope::encode
        } else {
            ProbeEnvelope::decode
        };
        Ok(call(input).map_err(|e| format!("{e:?}")))
    })
}
