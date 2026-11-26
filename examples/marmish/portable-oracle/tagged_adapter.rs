#[allow(dead_code)]
mod wire;

use std::io::{self, BufRead};
use wire::Value;

fn bytes(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn record(tag: u8, fields: Vec<Value>) -> String {
    let [Value::UInt(offset), Value::Bytes(data)] = fields.as_slice() else {
        panic!("unexpected generated Data fields");
    };
    assert_eq!(wire::schema().variant_name(tag), Some("Data"));
    format!("{tag}:{offset}:{}", hex(data))
}

fn main() {
    let schema = wire::schema();
    let tags: Vec<_> = (u8::MIN..=u8::MAX)
        .filter(|tag| schema.variant_name(*tag) == Some("Data"))
        .collect();
    let [tag] = tags.as_slice() else {
        panic!("expected one generated Data variant");
    };
    let mut decoder = schema.decoder().expect("incremental profile");
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let parts: Vec<_> = line.split(' ').collect();
        let result = match parts.as_slice() {
            ["E", offset, data] => schema
                .encode(
                    *tag,
                    &[
                        Value::UInt(offset.parse().unwrap()),
                        Value::Bytes(bytes(data)),
                    ],
                )
                .map(|encoded| format!("E:{}", hex(&encoded))),
            ["T", explicit_tag, offset, data] => schema
                .encode(
                    explicit_tag.parse().unwrap(),
                    &[
                        Value::UInt(offset.parse().unwrap()),
                        Value::Bytes(bytes(data)),
                    ],
                )
                .map(|encoded| format!("E:{}", hex(&encoded))),
            ["D", data] => schema
                .decode(&bytes(data))
                .map(|(tag, fields)| format!("D:{}", record(tag, fields))),
            ["B"] => {
                decoder = schema.decoder().unwrap();
                Ok("B".into())
            }
            ["F", data] => decoder.feed(&bytes(data)).map(|feed| match feed.record {
                Some((tag, fields)) => format!("F:{}:{}", feed.consumed, record(tag, fields)),
                None => format!("F:{}:", feed.consumed),
            }),
            ["X"] => decoder.finish().map(|()| "X".into()),
            _ => panic!("invalid adapter operation"),
        };
        match result {
            Ok(value) => println!("{value}"),
            Err(error) => println!("!{error}"),
        }
    }
}
