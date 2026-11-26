use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Workflow {
    pub(super) format: String,
    pub(super) id: String,
    pub(super) socket_argument: String,
    pub(super) discovery: Option<Value>,
    pub(super) profile_inputs: Option<Value>,
    #[serde(default)]
    pub(super) delivery: bool,
    pub(super) transport: Transport,
    pub(super) errors: std::collections::BTreeMap<String, Diagnostic>,
    pub(super) entry: String,
    pub(super) steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transport {
    pub(super) deadline_ms: u32,
    pub(super) maximum_frame_bytes: u32,
    pub(super) maximum_exchanges: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Diagnostic {
    pub(super) exit: u8,
    pub(super) stderr: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step {
    pub(super) id: String,
    pub(super) request: Vec<Field>,
    pub(super) embedded: Option<EmbeddedReply>,
    pub(super) reject: Option<Rejection>,
    pub(super) transitions: Vec<Transition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Field {
    pub(super) id: String,
    pub(super) key: String,
    pub(super) source: Source,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Source {
    Literal {
        value: Value,
    },
    Argument {
        id: String,
        value_kind: ArgumentKind,
    },
    Captured {
        name: String,
    },
    Input {
        name: String,
    },
    Object {
        fields: Vec<Field>,
    },
    Sha256 {
        fields: Vec<Field>,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ArgumentKind {
    String,
    RepeatedString,
    Unsigned64,
    Flag,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transition {
    pub(super) id: String,
    pub(super) when: Vec<Predicate>,
    #[serde(default)]
    pub(super) capture: Vec<Capture>,
    #[serde(default)]
    pub(super) carry: Vec<String>,
    #[serde(default)]
    pub(super) outputs: Vec<BinaryOutput>,
    #[serde(default)]
    pub(super) bindings: Vec<BindingCheck>,
    pub(super) fresh_id: Option<String>,
    pub(super) record: Option<Record>,
    #[serde(default)]
    pub(super) flags: std::collections::BTreeMap<String, bool>,
    pub(super) effect: Effect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Predicate {
    pub(super) path: Vec<String>,
    #[serde(default, deserialize_with = "present")]
    pub(super) equals: Option<Value>,
    pub(super) equals_source: Option<Source>,
    pub(super) equals_path: Option<Vec<String>>,
    pub(super) one_of: Option<Vec<Value>>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbeddedReply {
    pub(super) id: String,
    pub(super) bytes: Vec<String>,
    pub(super) sha256: Vec<String>,
    pub(super) maximum_bytes: u32,
    pub(super) tagged: Option<Tag>,
    #[serde(default)]
    pub(super) wire_check: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tag {
    pub(super) path: Vec<String>,
    pub(super) value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) id: String,
    pub(super) prefix: String,
    pub(super) source: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rejection {
    pub(super) id: String,
    pub(super) diagnostic: Diagnostic,
    pub(super) record: Option<Record>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BindingCheck {
    pub(super) id: String,
    pub(super) left: Source,
    pub(super) right: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BinaryOutput {
    pub(super) id: String,
    pub(super) channel: OutputChannel,
    pub(super) reply_capture: Option<String>,
    pub(super) bytes: Vec<String>,
    pub(super) cursor: Vec<String>,
    pub(super) start: Option<String>,
    pub(super) upper: Option<Vec<String>>,
    pub(super) maximum_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OutputChannel {
    Stdout,
    Stderr,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    pub(super) name: String,
    pub(super) path: Vec<String>,
    pub(super) value_kind: CaptureKind,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CaptureKind {
    LowerHex { bytes: u8 },
    String { maximum_bytes: u32 },
    Unsigned64,
    ExitStatus,
    JsonObject { maximum_bytes: u32 },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Effect {
    Continue { step: String, delay_ms: u32 },
    Exit { status: u8, stderr: String },
    ExitCaptured { name: String },
}
