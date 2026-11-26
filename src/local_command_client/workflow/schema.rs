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
    pub(super) effect: Effect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Predicate {
    pub(super) path: Vec<String>,
    pub(super) equals: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    pub(super) name: String,
    pub(super) path: Vec<String>,
    pub(super) value_kind: CaptureKind,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CaptureKind {
    LowerHex { bytes: u8 },
    String { maximum_bytes: u32 },
    Unsigned64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Effect {
    Continue { step: String, delay_ms: u32 },
    Exit { status: u8, stderr: String },
}
