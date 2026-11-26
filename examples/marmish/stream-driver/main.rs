use marmish_stream::{
    self as stream, Error,
    codec::{self, Codec, EncodedBlock},
    copy_protocol::{CopyError, Frame, FrameDecoder, Stage},
    windows::{ReceiveWindow, SendWindow},
    wire::{self, Binding, Request, RequestBody, Response, ResponseBody, WireErrorCode},
};
use serde_json::{Value, json};
use std::{fmt::Write, io::Read};

fn number(value: &Value) -> u64 {
    value.as_u64().expect("vector integer")
}

fn text(value: &Value) -> &str {
    value.as_str().expect("vector string")
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(result, "{byte:02x}").unwrap();
    }
    result
}

fn bytes(value: &Value) -> Vec<u8> {
    if let Some(string) = value.as_str() {
        assert_eq!(string.len() % 2, 0);
        return (0..string.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&string[i..i + 2], 16).unwrap())
            .collect();
    }
    let mut out = bytes(&value["prefix"]);
    let byte = u8::try_from(number(&value["byte"])).unwrap();
    let repeat = usize::try_from(number(&value["repeat"])).unwrap();
    out.extend(std::iter::repeat_n(byte, repeat));
    out
}

fn observed<T>(result: stream::Result<T>, success: impl FnOnce(T) -> Value) -> Value {
    match result {
        Ok(value) => success(value),
        Err(Error::Invalid(detail)) => json!({"error": "Invalid", "detail": detail}),
        Err(error) => json!({"error": format!("{error:?}")}),
    }
}

fn frame(value: &Value) -> Frame {
    match text(&value["kind"]) {
        "data" => Frame::Data {
            offset: number(&value["offset"]),
            bytes: bytes(&value["bytes"]),
        },
        "checkpoint" => Frame::Checkpoint {
            offset: number(&value["offset"]),
            sha256: bytes(&value["sha256"]).try_into().unwrap(),
        },
        "progress" => Frame::Progress {
            stage: Stage::PrefixHash,
            examined: number(&value["examined"]),
            total: number(&value["total"]),
        },
        "error" => Frame::Error(CopyError::HashMismatch),
        other => panic!("unknown frame {other}"),
    }
}

fn frame_value(frame: Frame) -> Value {
    match frame {
        Frame::Data { offset, bytes } => {
            json!({"kind": "data", "offset": offset, "bytes": hex(&bytes)})
        }
        Frame::Checkpoint { offset, sha256 } => {
            json!({"kind": "checkpoint", "offset": offset, "sha256": hex(&sha256)})
        }
        Frame::Progress {
            stage,
            examined,
            total,
        } => {
            json!({"kind": "progress", "stage": format!("{stage:?}"), "examined": examined, "total": total})
        }
        Frame::Error(code) => json!({"kind": "error", "code": format!("{code:?}")}),
        _ => panic!("unexpected decoded frame"),
    }
}

fn fragments(value: &Value) -> Value {
    let mut decoder = FrameDecoder::default();
    let steps = value.as_array().unwrap().iter().map(|step| {
        if step == "finish" {
            return observed(decoder.finish(), |()| json!("ok"));
        }
        observed(
            decoder.feed(&bytes(step)),
            |(consumed, frame)| json!({"consumed": consumed, "frame": frame.map(frame_value)}),
        )
    });
    Value::Array(steps.collect())
}

fn send_window(steps: &[Value]) -> Value {
    let mut window = SendWindow::default();
    Value::Array(
        steps
            .iter()
            .map(|step| match text(&step["op"]) {
                "push" => observed(window.push(&bytes(&step["bytes"])), |offset| json!(offset)),
                "read" => observed(
                    window.read(
                        number(&step["offset"]),
                        usize::try_from(number(&step["limit"])).unwrap(),
                    ),
                    |data| json!(hex(&data)),
                ),
                "ack" => observed(window.acknowledge(number(&step["offset"])), |()| {
                    json!("ok")
                }),
                "finish" => observed(window.finish(number(&step["offset"])), |()| json!("ok")),
                "state" => json!({
                    "produced": window.produced(), "acknowledged": window.acknowledged(),
                    "buffered": window.buffered(), "credit": window.credit(),
                    "eof": window.eof(), "drained": window.drained()
                }),
                other => panic!("unknown send step {other}"),
            })
            .collect(),
    )
}

fn receive_window(steps: &[Value]) -> Value {
    let mut window = ReceiveWindow::default();
    Value::Array(
        steps
            .iter()
            .map(|step| match text(&step["op"]) {
                "accept" => observed(
                    window.accept(number(&step["offset"]), &bytes(&step["bytes"])),
                    |new| json!(new),
                ),
                "pending" => observed(
                    window.pending(usize::try_from(number(&step["limit"])).unwrap()),
                    |bytes| json!(hex(&bytes)),
                ),
                "deliver" => observed(window.delivered_through(number(&step["offset"])), |()| {
                    json!("ok")
                }),
                "finish" => observed(window.finish(number(&step["offset"])), |()| json!("ok")),
                "state" => json!({
                    "accepted": window.accepted(), "delivered": window.delivered(),
                    "buffered": window.buffered(), "credit": window.credit(),
                    "eof": window.eof(), "drained": window.drained()
                }),
                other => panic!("unknown receive step {other}"),
            })
            .collect(),
    )
}

fn binding(value: &Value) -> Binding {
    Binding {
        stream: bytes(&value["stream"]).try_into().unwrap(),
        session: bytes(&value["session"]).try_into().unwrap(),
        client: bytes(&value["client"]).try_into().unwrap(),
        server: bytes(&value["server"]).try_into().unwrap(),
    }
}

fn request_value(request: Request) -> Value {
    json!({
        "binding": request.binding, "sequence": request.sequence, "body": request.body,
    })
}

fn response_value(response: Response) -> Value {
    json!({
        "binding": response.binding, "sequence": response.sequence, "body": response.body,
        "request_sha256": hex(&response.request_sha256),
    })
}

fn selected_codec(value: &Value) -> Codec {
    match text(value) {
        "none" => Codec::None,
        "zlib" => Codec::Zlib,
        other => panic!("unknown codec {other}"),
    }
}

fn execute(case: &Value) -> Value {
    let input = &case["input"];
    match text(&case["operation"]) {
        "frame_encode" => observed(frame(input).encode(), |bytes| json!(hex(&bytes))),
        "frame_decode" => observed(Frame::decode(&bytes(input)), frame_value),
        "frame_fragments" => fragments(input),
        "request_encode" => observed(
            wire::encode_request(&Request {
                binding: binding(&input["binding"]),
                sequence: number(&input["sequence"]),
                body: RequestBody::Hello {
                    codecs: vec![Codec::None, Codec::Zlib],
                },
            }),
            |bytes| json!(String::from_utf8(bytes).unwrap()),
        ),
        "request_decode" => observed(wire::decode_request(text(input).as_bytes()), request_value),
        "request_digest" => json!(hex(&wire::request_digest(text(input).as_bytes()))),
        "response_encode" => observed(
            wire::encode_response(&Response {
                binding: binding(&input["binding"]),
                sequence: number(&input["sequence"]),
                request_sha256: bytes(&input["request_sha256"]).try_into().unwrap(),
                body: ResponseBody::Error {
                    code: WireErrorCode::Gap,
                },
            }),
            |bytes| json!(String::from_utf8(bytes).unwrap()),
        ),
        "response_decode" => observed(
            wire::decode_response(text(input).as_bytes()),
            response_value,
        ),
        "codec_encode" => observed(
            codec::encode(
                number(&input["offset"]),
                &bytes(&input["bytes"]),
                selected_codec(&input["negotiated"]),
            ),
            |block| json!(block),
        ),
        "codec_decode" => {
            let block: EncodedBlock =
                serde_json::from_value(input["block"].clone()).expect("codec vector block");
            observed(
                codec::decode(&block, selected_codec(&input["negotiated"])),
                |bytes| json!(hex(&bytes)),
            )
        }
        "send_window" => send_window(input.as_array().unwrap()),
        "receive_window" => receive_window(input.as_array().unwrap()),
        other => panic!("unknown operation {other}"),
    }
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let cases: Value = serde_json::from_str(&input).unwrap();
    let results: Vec<_> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|case| json!({"name": case["name"], "actual": execute(case)}))
        .collect();
    println!(
        "{}",
        json!({
            "role": env!("ODDLANG_STREAM_DRIVER_ROLE"),
            "binding": env!("ODDLANG_STREAM_DRIVER_BINDING"),
            "cases": results
        })
    );
}
