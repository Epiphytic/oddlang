use super::schema::ArgumentKind;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileInputs {
    pub(crate) id: String,
    pub(crate) arguments: InputArguments,
    pub(crate) document: InputDocument,
    pub(crate) payload: Payload,
    pub(crate) outputs: OutputNames,
    pub(crate) resume: Option<Resume>,
    pub(crate) errors: BTreeMap<String, Diagnostic>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputArguments {
    pub(crate) state: String,
    pub(crate) profile: String,
    pub(crate) default_profile: String,
    pub(crate) relay: String,
    pub(crate) route: String,
    pub(crate) stdin: String,
    pub(crate) stdin_none: String,
    pub(crate) required_flags: Vec<String>,
    pub(crate) excluded_flags: Vec<String>,
    pub(crate) excluded_values: Vec<ExcludedArgument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExcludedArgument {
    pub(crate) id: String,
    pub(crate) value_kind: ArgumentKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputDocument {
    pub(crate) leaf: String,
    pub(crate) maximum_bytes: u32,
    pub(crate) directory_mode: u32,
    pub(crate) file_mode: u32,
    pub(crate) profile_name_maximum_bytes: u16,
    pub(crate) profile_name_punctuation: String,
    pub(crate) checks: Vec<InputCheck>,
    pub(crate) trusted: String,
    pub(crate) maximum_accounts: usize,
    pub(crate) relay: String,
    pub(crate) route: String,
    pub(crate) maximum_relays: usize,
    pub(crate) maximum_relay_bytes: usize,
    pub(crate) relay_rules: Vec<RelayRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputCheck {
    pub(crate) id: String,
    pub(crate) field: String,
    pub(crate) test: InputTest,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InputTest {
    OneOf { values: Vec<Value> },
    Object,
    NullOrAbsolutePath,
    AccountSet,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelayRule {
    pub(crate) id: String,
    pub(crate) scheme: String,
    pub(crate) host_suffix: Option<String>,
    #[serde(default)]
    pub(crate) v3_onion: bool,
    pub(crate) routes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    pub(crate) maximum_arguments: usize,
    pub(crate) maximum_argument_bytes: usize,
    pub(crate) maximum_stdin_bytes: usize,
    pub(crate) stdin_timeout_ms: u32,
    pub(crate) timestamp_bytes: u8,
    pub(crate) random_bytes: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputNames {
    pub(crate) remote: String,
    pub(crate) relays: String,
    pub(crate) argv: String,
    pub(crate) stdin: String,
    pub(crate) request: String,
    pub(crate) invocation: String,
    pub(crate) selected_profile: Option<String>,
    pub(crate) followup_request: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Diagnostic {
    pub(crate) exit: u8,
    pub(crate) stderr: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resume {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) operation: String,
    pub(crate) handle_argument: String,
    pub(crate) maximum_handle_bytes: u32,
    pub(crate) handle_fields: HandleFields,
    pub(crate) manifest: AccountManifest,
    pub(crate) local_account_output: String,
    pub(crate) session_output: String,
    pub(crate) digest_output: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HandleFields {
    pub(crate) version_key: String,
    pub(crate) version: u8,
    pub(crate) profile: String,
    pub(crate) remote: String,
    pub(crate) session: String,
    pub(crate) invocation: String,
    pub(crate) digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccountManifest {
    pub(crate) leaf: String,
    pub(crate) maximum_bytes: u32,
    pub(crate) version_key: String,
    pub(crate) version: u8,
    pub(crate) name_key: String,
    pub(crate) phase_key: String,
    pub(crate) ready: String,
    pub(crate) account_key: String,
}
