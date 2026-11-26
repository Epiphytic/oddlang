// Independent external user of the public, offline core API.
use marmish_core::{ConfigError, FrameError, Limits, ProbeEnvelope, RelaySet, Retention};
use std::{
    env,
    io::{self, Read, Write},
};

fn frame_error(error: FrameError) -> &'static str {
    match error {
        FrameError::SizeLimit => "SizeLimit",
        FrameError::InvalidJson => "InvalidJson",
        FrameError::InvalidVersion => "InvalidVersion",
        FrameError::InvalidEncoding => "InvalidEncoding",
    }
}

fn config_error(error: ConfigError) -> &'static str {
    match error {
        ConfigError::EmptyRelaySet => "EmptyRelaySet",
        ConfigError::InvalidUrl => "InvalidUrl",
        ConfigError::DisallowedScheme => "DisallowedScheme",
        ConfigError::DisallowedComponents => "DisallowedComponents",
        ConfigError::DisallowedHost => "DisallowedHost",
        ConfigError::LoopbackDenied => "LoopbackDenied",
        ConfigError::RetentionOutOfRange => "RetentionOutOfRange",
    }
}

fn api() {
    assert_eq!(Limits::RAW_BYTES, 16_384);
    assert_eq!(Limits::JSON_BYTES, 32_768);
    assert_eq!(Limits::NDJSON_BYTES, 65_536);
    assert_eq!(Retention::MAX_SECONDS, 31_536_000);
    assert_eq!(Retention::default().seconds(), Some(86_400));
    assert_eq!(Retention::from_seconds(None).unwrap().seconds(), None);
    for value in [1, Retention::MAX_SECONDS] {
        assert_eq!(
            Retention::from_seconds(Some(value)).unwrap().seconds(),
            Some(value)
        );
    }
    assert_eq!(
        config_error(Retention::from_seconds(Some(0)).err().unwrap()),
        "RetentionOutOfRange"
    );
    let relays = RelaySet::parse(
        &["WSS://EXAMPLE.COM:443".into(), "wss://example.com/".into()],
        false,
    )
    .unwrap();
    assert_eq!(relays.as_slice(), ["wss://example.com/"]);
    assert_eq!(
        config_error(RelaySet::parse(&[], false).err().unwrap()),
        "EmptyRelaySet"
    );
}

fn main() {
    let operation = env::args().nth(1).unwrap();
    if operation == "api" {
        api();
        return;
    }
    let call: fn(&[u8]) -> Result<Vec<u8>, FrameError> = match operation.as_str() {
        "encode" => ProbeEnvelope::encode,
        "decode" => ProbeEnvelope::decode,
        _ => panic!("unknown observation"),
    };
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).unwrap();
    match call(&input) {
        Ok(bytes) => io::stdout().write_all(&bytes).unwrap(),
        Err(error) => {
            eprintln!("{}", frame_error(error));
            std::process::exit(2);
        }
    }
}
