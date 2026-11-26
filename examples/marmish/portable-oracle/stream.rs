// The phase-one adapter extends the archived Rust caller without modifying it.
use serde_json::{Value, json};
use std::io::Read;

#[allow(dead_code)]
mod adapter {
    include!("../stream-driver/main.rs");

    use stream::copy_protocol::{
        self as copy, AttemptMode, Checkpoint, Direction, FileIdentity, HelperContext, Launch,
        Manifest, Mtime, Phase,
    };

    fn parsed<T: serde::de::DeserializeOwned>(value: &Value) -> T {
        serde_json::from_value(value.clone()).expect("typed vector input")
    }

    fn fixed<const N: usize>(value: &Value) -> [u8; N] {
        bytes(value).try_into().expect("fixed-width vector input")
    }

    fn small(value: &Value) -> u8 {
        number(value).try_into().unwrap()
    }

    fn u32_value(value: &Value) -> u32 {
        number(value).try_into().unwrap()
    }

    fn signed(value: &Value) -> i64 {
        value.as_i64().unwrap()
    }

    fn unit((): ()) -> Value {
        json!("ok")
    }

    fn byte_value(value: impl AsRef<[u8]>) -> Value {
        json!(hex(value.as_ref()))
    }

    fn manifest(value: &Value) -> Manifest {
        let launch_fields = parsed::<Launch>(&json!({
            "version": value["version"], "mode": "create", "transfer": value["transfer"],
            "client": value["client"], "server": value["server"], "direction": value["direction"],
            "source": value["source"], "destination": value["destination"], "overwrite": value["overwrite"],
            "preserve_mode": value["preserve_mode"], "preserve_mtime": value["preserve_mtime"],
        }));
        Manifest {
            version: launch_fields.version,
            transfer: launch_fields.transfer,
            client: launch_fields.client,
            server: launch_fields.server,
            direction: launch_fields.direction,
            source: launch_fields.source,
            destination: launch_fields.destination,
            overwrite: launch_fields.overwrite,
            preserve_mode: launch_fields.preserve_mode,
            preserve_mtime: launch_fields.preserve_mtime,
            size: number(&value["size"]),
            sha256: fixed(&value["sha256"]),
            mode: parsed(&value["mode"]),
            mtime: (!value["mtime"].is_null()).then(|| Mtime {
                seconds: signed(&value["mtime"]["seconds"]),
                nanos: u32_value(&value["mtime"]["nanos"]),
            }),
        }
    }

    fn manifest_value(value: Manifest) -> Value {
        let mut shape = serde_json::to_value(Launch {
            version: value.version,
            mode: AttemptMode::Create,
            transfer: value.transfer,
            client: value.client,
            server: value.server,
            direction: value.direction,
            source: value.source,
            destination: value.destination,
            overwrite: value.overwrite,
            preserve_mode: value.preserve_mode,
            preserve_mtime: value.preserve_mtime,
        })
        .unwrap();
        shape.as_object_mut().unwrap().remove("mode");
        shape["size"] = json!(value.size);
        shape["sha256"] = byte_value(value.sha256);
        shape["mode"] = json!(value.mode);
        shape["mtime"] = json!(
            value
                .mtime
                .map(|time| json!({"seconds": time.seconds, "nanos": time.nanos}))
        );
        shape
    }

    fn identity(value: &Value) -> FileIdentity {
        FileIdentity {
            device: number(&value["device"]),
            inode: number(&value["inode"]),
        }
    }

    fn identity_value(value: FileIdentity) -> Value {
        json!({"device": value.device, "inode": value.inode})
    }

    fn checkpoint(value: &Value) -> Checkpoint {
        Checkpoint {
            manifest: manifest(&value["manifest"]),
            phase: Phase::try_from(small(&value["phase"])).unwrap(),
            normalized_path: bytes(&value["normalized_path"]),
            parent: identity(&value["parent"]),
            private: identity(&value["private"]),
            data: identity(&value["data"]),
            durable_offset: number(&value["durable_offset"]),
            prefix_sha256: fixed(&value["prefix_sha256"]),
            last_stream: fixed(&value["last_stream"]),
            created: signed(&value["created"]),
            updated: signed(&value["updated"]),
            expires: signed(&value["expires"]),
            committed_final: (!value["committed_final"].is_null())
                .then(|| identity(&value["committed_final"])),
        }
    }

    fn checkpoint_value(value: Checkpoint) -> Value {
        json!({
            "manifest": manifest_value(value.manifest), "phase": value.phase as u8,
            "normalized_path": byte_value(value.normalized_path),
            "parent": identity_value(value.parent), "private": identity_value(value.private),
            "data": identity_value(value.data), "durable_offset": value.durable_offset,
            "prefix_sha256": byte_value(value.prefix_sha256), "last_stream": byte_value(value.last_stream),
            "created": value.created, "updated": value.updated, "expires": value.expires,
            "committed_final": value.committed_final.map(identity_value),
        })
    }

    fn context(value: &Value) -> HelperContext {
        HelperContext {
            context_version: u32_value(&value["context_version"]),
            helper_protocol: u32_value(&value["helper_protocol"]),
            client: fixed(&value["client"]),
            server: fixed(&value["server"]),
            group: bytes(&value["group"]),
            session: fixed(&value["session"]),
            stream: fixed(&value["stream"]),
            transfer: fixed(&value["transfer"]),
            direction: Direction::try_from(small(&value["direction"])).unwrap(),
            launch_digest: fixed(&value["launch_digest"]),
            retention_seconds: number(&value["retention_seconds"]),
            expected_version: text(&value["expected_version"]).to_owned(),
            expected_target: text(&value["expected_target"]).to_owned(),
            expected_commit: fixed(&value["expected_commit"]),
        }
    }

    fn context_value(value: HelperContext) -> Value {
        json!({
            "context_version": value.context_version, "helper_protocol": value.helper_protocol,
            "client": byte_value(value.client), "server": byte_value(value.server),
            "group": byte_value(value.group), "session": byte_value(value.session),
            "stream": byte_value(value.stream), "transfer": byte_value(value.transfer),
            "direction": value.direction as u8, "launch_digest": byte_value(value.launch_digest),
            "retention_seconds": value.retention_seconds, "expected_version": value.expected_version,
            "expected_target": value.expected_target, "expected_commit": byte_value(value.expected_commit),
        })
    }

    fn copy_frame(value: &Value) -> Frame {
        match text(&value["kind"]) {
            "start" => Frame::Start(parsed(&value["launch"]), selected_codec(&value["codec"])),
            "manifest" => Frame::Manifest(manifest(&value["manifest"])),
            "resume" => Frame::Resume {
                phase: Phase::try_from(small(&value["phase"])).unwrap(),
                offset: number(&value["offset"]),
                sha256: fixed(&value["sha256"]),
            },
            "prefix" => Frame::Prefix {
                offset: number(&value["offset"]),
                sha256: fixed(&value["sha256"]),
            },
            "done" => Frame::Done {
                size: number(&value["size"]),
                sha256: fixed(&value["sha256"]),
            },
            "commit" => Frame::Commit {
                transfer: fixed(&value["transfer"]),
                size: number(&value["size"]),
                sha256: fixed(&value["sha256"]),
            },
            "finish" => Frame::Finish {
                transfer: fixed(&value["transfer"]),
                sha256: fixed(&value["sha256"]),
            },
            _ => frame(value),
        }
    }

    fn copy_frame_value(value: Frame) -> Value {
        match value {
            Frame::Start(launch, codec) => {
                json!({"kind": "start", "launch": launch, "codec": codec})
            }
            Frame::Manifest(record) => {
                json!({"kind": "manifest", "manifest": manifest_value(record)})
            }
            Frame::Resume {
                phase,
                offset,
                sha256,
            } => json!({
                "kind": "resume", "phase": phase as u8, "offset": offset, "sha256": byte_value(sha256),
            }),
            Frame::Prefix { offset, sha256 } => json!({
                "kind": "prefix", "offset": offset, "sha256": byte_value(sha256),
            }),
            Frame::Done { size, sha256 } => {
                json!({"kind": "done", "size": size, "sha256": byte_value(sha256)})
            }
            Frame::Commit {
                transfer,
                size,
                sha256,
            } => json!({
                "kind": "commit", "transfer": byte_value(transfer), "size": size, "sha256": byte_value(sha256),
            }),
            Frame::Finish { transfer, sha256 } => json!({
                "kind": "finish", "transfer": byte_value(transfer), "sha256": byte_value(sha256),
            }),
            other => frame_value(other),
        }
    }

    fn request(input: &Value) -> Request {
        Request {
            binding: binding(&input["binding"]),
            sequence: number(&input["sequence"]),
            body: parsed(&input["body"]),
        }
    }

    fn response(input: &Value) -> Response {
        Response {
            binding: binding(&input["binding"]),
            sequence: number(&input["sequence"]),
            body: parsed(&input["body"]),
            request_sha256: fixed(&input["request_sha256"]),
        }
    }

    fn constants() -> Value {
        json!({
            "MAX_BLOCK": stream::MAX_BLOCK, "MAX_ENCODED": stream::MAX_ENCODED, "MAX_CONTENT": stream::MAX_CONTENT,
            "WINDOW_BYTES": stream::WINDOW_BYTES, "MAX_OFFSET": stream::MAX_OFFSET, "MAX_RECORDS": stream::MAX_RECORDS,
            "PROTOCOL": wire::PROTOCOL, "FORMAT": wire::FORMAT, "VERSION": copy::VERSION, "MAX_PATH": copy::MAX_PATH,
            "MAX_CONTROL": copy::MAX_CONTROL, "DURABLE_WINDOW": copy::DURABLE_WINDOW,
            "CHECKPOINT_INTERVAL": copy::CHECKPOINT_INTERVAL, "MIN_RETENTION_SECS": copy::MIN_RETENTION_SECS,
            "MAX_RETENTION_SECS": copy::MAX_RETENTION_SECS, "CONTEXT_VERSION": copy::CONTEXT_VERSION,
            "MAX_CONTEXT": copy::MAX_CONTEXT, "HELPER_ENTRYPOINT": copy::HELPER_ENTRYPOINT,
        })
    }

    fn converted<T: TryFrom<u8, Error = Error> + std::fmt::Debug>() -> Vec<Value> {
        (u8::MIN..=u8::MAX)
            .map(|n| observed(T::try_from(n), |value| json!(format!("{value:?}"))))
            .collect()
    }

    pub(super) fn observe(case: &Value) -> Value {
        let input = &case["input"];
        match text(&case["operation"]) {
            "frame_encode" => observed(copy_frame(input).encode(), byte_value),
            "frame_decode" => observed(Frame::decode(&bytes(input)), copy_frame_value),
            "request_encode" if input.get("body").is_some() => {
                observed(wire::encode_request(&request(input)), |data| {
                    json!(String::from_utf8(data).unwrap())
                })
            }
            "response_encode" if input.get("body").is_some() => {
                observed(wire::encode_response(&response(input)), |data| {
                    json!(String::from_utf8(data).unwrap())
                })
            }
            "request_age" => observed(request(input).validate_at(number(&input["now"])), unit),
            "open_digest" => {
                let argv: Vec<_> = input["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(bytes)
                    .collect();
                observed(
                    wire::open_digest(&argv, selected_codec(&input["codec"])),
                    byte_value,
                )
            }
            "launch_validate" => observed(parsed::<Launch>(input).validate(), unit),
            "launch_digest" => observed(
                parsed::<Launch>(&input["launch"]).digest(selected_codec(&input["codec"])),
                byte_value,
            ),
            "launch_namespace" => json!(parsed::<Launch>(input).namespace()),
            "launch_matches_manifest" => {
                json!(
                    parsed::<Launch>(&input["launch"])
                        .matches_manifest(&manifest(&input["manifest"]))
                )
            }
            "manifest_validate" => observed(manifest(input).validate(), unit),
            "manifest_encode" => observed(manifest(input).encode(), byte_value),
            "manifest_decode" => observed(Manifest::decode(&bytes(input)), manifest_value),
            "manifest_namespace" => json!(manifest(input).namespace()),
            "checkpoint_validate" => observed(checkpoint(input).validate(), unit),
            "checkpoint_encode" => observed(checkpoint(input).encode(), byte_value),
            "checkpoint_decode" => observed(Checkpoint::decode(&bytes(input)), checkpoint_value),
            "context_validate" => observed(context(input).validate(), unit),
            "context_encode" => observed(context(input).encode(), byte_value),
            "context_decode" => observed(HelperContext::decode(&bytes(input)), context_value),
            "context_start" => observed(
                context(&input["context"])
                    .validate_start(&parsed(&input["launch"]), selected_codec(&input["codec"])),
                unit,
            ),
            "context_build" => observed(
                context(&input["context"]).validate_build(
                    text(&input["version"]),
                    text(&input["target"]),
                    fixed(&input["commit"]),
                ),
                unit,
            ),
            "codec_end" => observed(parsed::<EncodedBlock>(input).end(), |end| json!(end)),
            "codec_validate" => observed(parsed::<EncodedBlock>(input).validate(), unit),
            "constants" => constants(),
            "enum_conversions" => json!({
                "AttemptMode": converted::<AttemptMode>(), "Direction": converted::<Direction>(),
                "Phase": converted::<Phase>(), "Stage": converted::<Stage>(), "CopyError": converted::<CopyError>(),
            }),
            _ => execute(case),
        }
    }
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&input).unwrap();
    let observations: Vec<_> = cases
        .iter()
        .map(|case| json!({"name": case["name"], "actual": adapter::observe(case)}))
        .collect();
    println!(
        "{}",
        json!({
            "binding": env!("ODDLANG_STREAM_DRIVER_BINDING"),
            "cases": observations,
        })
    );
}
