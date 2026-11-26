//! Compile read-only descriptor policies for inert Unix files.
use crate::{
    native_package::publish,
    wire_schema::{digest, encode, iri, symbol},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Graph {
    format: String,
    maximum_links: u64,
    profile: Profile,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    id: String,
    mode_mask: u32,
    digest: Digest,
    open: Vec<String>,
    guards: Vec<String>,
    read_ceiling: String,
    safety: Safety,
    errors: Errors,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Safety {
    preflight_file_type: String,
    nonblocking_open: bool,
    hash_ceiling: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Digest {
    algorithm: String,
    chunk_bytes: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Errors {
    io: IoError,
    descriptor: Rejection,
    bounded_read: Rejection,
    metadata: Rejection,
    digest: Rejection,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IoError {
    id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Rejection {
    id: String,
    code: String,
}

impl Graph {
    fn validate(&self) -> Result<()> {
        let profile = &self.profile;
        iri(&profile.id)?;
        iri(&profile.errors.io.id)?;
        let rejections = [
            &profile.errors.descriptor,
            &profile.errors.bounded_read,
            &profile.errors.metadata,
            &profile.errors.digest,
        ];
        for error in rejections {
            iri(&error.id)?;
        }
        if self.format != "oddlang-read-only-file-v1"
            || self.maximum_links == 0
            || profile.mode_mask == 0
            || profile.mode_mask > 0o777
            || profile.digest.algorithm != "sha256"
            || !(1..=1024 * 1024).contains(&profile.digest.chunk_bytes)
            || profile.open != ["read-only", "no-follow-final-link", "close-on-exec"]
            || profile.guards
                != [
                    "regular",
                    "links",
                    "maximum-size",
                    "exact-size",
                    "effective-owner",
                    "masked-mode",
                    "digest",
                ]
            || profile.read_ceiling != "requested-size-plus-one"
            || profile.safety.preflight_file_type != "regular-or-final-symlink"
            || !profile.safety.nonblocking_open
            || profile.safety.hash_ceiling != "exact-size-plus-one"
            || rejections
                .iter()
                .map(|error| &error.code)
                .collect::<BTreeSet<_>>()
                .len()
                != 4
            || rejections.iter().any(|error| {
                error.id != profile.errors.descriptor.id
                    || error.id == profile.errors.io.id
                    || error.code.is_empty()
                    || !error.code.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_')
                    })
            })
        {
            return Err("unsupported read-only file descriptor policy".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    open: String,
    read: String,
    verify: String,
    error: Vec<String>,
    invalid_variant: String,
    io_variant: String,
}

impl Binding {
    fn validate(&self) -> Result<()> {
        let operations = [&self.open, &self.read, &self.verify];
        if self.format != "oddlang-read-only-file-rust-v1"
            || operations.into_iter().collect::<BTreeSet<_>>().len() != 3
            || operations.into_iter().any(|name| !symbol(name))
            || self.error.len() < 2
            || self.error.first().is_none_or(|root| root != "crate")
            || self.error[1..].iter().any(|name| !symbol(name))
            || !symbol(&self.invalid_variant)
            || !symbol(&self.io_variant)
            || self.invalid_variant == self.io_variant
        {
            return Err("invalid read-only file Rust binding".into());
        }
        Ok(())
    }
}

pub(crate) struct NativeLink {
    pub id: String,
    pub error: Vec<String>,
    pub variant: String,
    pub io: bool,
}

pub(crate) fn identity(raw: &[u8]) -> Result<String> {
    let graph: Graph = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    Ok(graph.profile.id)
}

pub(crate) fn native_links(graph: &[u8], binding: &[u8]) -> Result<([String; 3], [NativeLink; 2])> {
    let graph: Graph = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let binding: Binding = serde_json::from_slice(binding).map_err(|error| error.to_string())?;
    graph.validate()?;
    binding.validate()?;
    Ok((
        [binding.open, binding.read, binding.verify],
        [
            NativeLink {
                id: graph.profile.errors.io.id,
                error: binding.error.clone(),
                variant: binding.io_variant,
                io: true,
            },
            NativeLink {
                id: graph.profile.errors.descriptor.id,
                error: binding.error,
                variant: binding.invalid_variant,
                io: false,
            },
        ],
    ))
}

fn stage(path: &Path) -> Result<tempfile::TempDir> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    tempfile::tempdir_in(parent).map_err(|error| error.to_string())
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let raw = fs::read(graph).map_err(|error| error.to_string())?;
    let native = fs::read(overlay).map_err(|error| error.to_string())?;
    let graph: Graph = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let binding: Binding = serde_json::from_slice(&native).map_err(|error| error.to_string())?;
    graph.validate()?;
    binding.validate()?;
    let errors = &graph.profile.errors;
    let error_type = binding.error.join("::");
    let rust = include_str!("file.rs.txt")
        .replace("@OPEN@", &binding.open)
        .replace("@READ@", &binding.read)
        .replace("@VERIFY@", &binding.verify)
        .replace("@ERROR@", &error_type)
        .replace("@INVALID@", &binding.invalid_variant)
        .replace("@IO@", &binding.io_variant)
        .replace("@MAX_LINKS@", &graph.maximum_links.to_string())
        .replace("@MODE_MASK@", &graph.profile.mode_mask.to_string())
        .replace("@CHUNK@", &graph.profile.digest.chunk_bytes.to_string())
        .replace("@UNSAFE@", &errors.descriptor.code)
        .replace("@BOUNDED@", &errors.bounded_read.code)
        .replace("@METADATA@", &errors.metadata.code)
        .replace("@DIGEST@", &errors.digest.code);
    let seal = encode(&serde_json::json!({
        "format": "oddlang-read-only-file-rust-lock-v1",
        "graphSha256": digest(&raw), "overlaySha256": digest(&native), "sourceSha256": digest(rust.as_bytes()),
    }))?;
    let stage = stage(output)?;
    fs::write(stage.path().join("file.rs"), rust).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("projection.lock"), seal).map_err(|error| error.to_string())?;
    publish(stage.path(), output)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    observation_sha256: String,
    profile: Profile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    format: String,
    one_link: String,
    two_links: String,
    three_links: String,
}

pub fn recover(observation: &Path, selection: &Path, output: &Path) -> Result<()> {
    let selection_raw = fs::read(selection).map_err(|error| error.to_string())?;
    let selection: Selection =
        serde_json::from_slice(&selection_raw).map_err(|error| error.to_string())?;
    let observation_raw = fs::read(observation).map_err(|error| error.to_string())?;
    if selection.format != "oddlang-read-only-file-selection-v1"
        || digest(&observation_raw) != selection.observation_sha256
    {
        return Err("read-only file observation pin mismatch".into());
    }
    let observed: Observation =
        serde_json::from_slice(&observation_raw).map_err(|error| error.to_string())?;
    if observed.format != "oddlang-read-only-file-observation-v1" {
        return Err("unrecognized descriptor observation format".into());
    }
    let maximum_links = match (
        observed.one_link.as_str(),
        observed.two_links.as_str(),
        observed.three_links.as_str(),
    ) {
        ("accepted", "descriptor", "descriptor") => 1,
        ("accepted", "accepted", "descriptor") => 2,
        _ => return Err("unrecognized bounded link observations".into()),
    };
    let graph = Graph {
        format: "oddlang-read-only-file-v1".into(),
        maximum_links,
        profile: selection.profile,
    };
    graph.validate()?;
    let bytes = encode(&graph)?;
    let seal = encode(&serde_json::json!({
        "format": "oddlang-read-only-file-recovery-lock-v1",
        "selectionSha256": digest(&selection_raw), "observationSha256": selection.observation_sha256,
        "graphSha256": digest(&bytes), "recovered": ["maximumLinks"]
    }))?;
    let stage = stage(output)?;
    fs::write(stage.path().join("graph.json"), bytes).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("recovery.lock"), seal).map_err(|error| error.to_string())?;
    publish(stage.path(), output)
}
