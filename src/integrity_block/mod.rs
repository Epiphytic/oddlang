//! Project bounded, positioned byte blocks with independent integrity validation.
use oxigraph::model::NamedNode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod obligations;
mod render;

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-integrity-block-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
enum Role {
    Offset,
    Representation,
    RawLength,
    Digest,
    Payload,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Check {
    RawSize,
    EncodedSize,
    Offset,
    RepresentationSize,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Graph {
    vocabulary: String,
    id: String,
    fields: Vec<Field>,
    bounds: Bounds,
    digest: String,
    representations: Representations,
    wire: Wire,
    errors: Errors,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    error_messages: BTreeMap<String, String>,
    validate_order: Vec<Check>,
    decode_order: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    provider_hints: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Field {
    role: Role,
    wire_name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bounds {
    raw_minimum: u32,
    raw_maximum: u32,
    encoded_minimum: u32,
    encoded_maximum: u32,
    offset_maximum: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Representations {
    identity: String,
    compressed: Option<Compressed>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Compressed {
    wire_name: String,
    algorithm: String,
    effort: String,
    selection: String,
    members: String,
    dictionary: String,
    trailing: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    shape: String,
    digest: String,
    payload: String,
    digest_syntax_message: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Errors {
    size: String,
    offset: String,
    representation: String,
    payload_syntax: String,
    payload_decoded_size: String,
}

impl Errors {
    fn roles(&self) -> [(&'static str, &str); 5] {
        [
            ("Size", &self.size),
            ("Offset", &self.offset),
            ("Representation", &self.representation),
            ("PayloadSyntax", &self.payload_syntax),
            ("PayloadDecodedSize", &self.payload_decoded_size),
        ]
    }
}

impl Graph {
    fn validate(&self) -> Result<()> {
        if self.vocabulary != FORMAT {
            return Err("unsupported integrity-block vocabulary".into());
        }
        if self.fields.len() != 5
            || self
                .fields
                .iter()
                .map(|row| row.role)
                .collect::<BTreeSet<_>>()
                .len()
                != 5
        {
            return Err("block needs exactly one field for each semantic role".into());
        }
        if self.fields.iter().any(|row| row.wire_name.is_empty())
            || self
                .fields
                .iter()
                .map(|row| &row.wire_name)
                .collect::<BTreeSet<_>>()
                .len()
                != 5
        {
            return Err("block fields require nonempty distinct wire names".into());
        }
        for id in std::iter::once(self.id.as_str()).chain(self.errors.roles().map(|(_, id)| id)) {
            NamedNode::new(id).map_err(|error| error.to_string())?;
        }
        let errors = self.errors.roles().map(|(_, id)| id);
        if self
            .error_messages
            .keys()
            .any(|id| !errors.contains(&id.as_str()))
        {
            return Err("wire messages must refer to declared error identities".into());
        }
        let bounds = &self.bounds;
        if bounds.raw_minimum > bounds.raw_maximum
            || bounds.encoded_minimum > bounds.encoded_maximum
            || bounds.raw_maximum >= i32::MAX as u32
            || bounds.encoded_maximum.div_ceil(3) > i32::MAX as u32 / 4
        {
            return Err("invalid or unrepresentable byte bounds".into());
        }
        if self.digest != "sha256"
            || self.wire.digest != "lower-hex"
            || self.wire.payload != "base64-standard-padded"
            || self.wire.shape != "closed-object-or-tuple"
        {
            return Err("unsupported digest or wire primitive".into());
        }
        if self.representations.identity.is_empty() {
            return Err("identity wire name is empty".into());
        }
        if let Some(compressed) = &self.representations.compressed {
            if compressed.wire_name.is_empty()
                || compressed.wire_name == self.representations.identity
            {
                return Err(
                    "compressed representation must have a distinct nonempty wire name".into(),
                );
            }
            if [
                compressed.algorithm.as_str(),
                &compressed.effort,
                &compressed.selection,
                &compressed.members,
                &compressed.dictionary,
                &compressed.trailing,
            ] != [
                "zlib",
                "default",
                "strictly-smaller",
                "exactly-one",
                "forbidden",
                "forbidden",
            ] {
                return Err("unsupported compression policy".into());
            }
        }
        if self.validate_order.len() != 4
            || self.validate_order.iter().collect::<BTreeSet<_>>().len() != 4
        {
            return Err("validation precedence must contain every check exactly once".into());
        }
        if self.decode_order
            != [
                "negotiated",
                "dictionary",
                "member",
                "decoded-size",
                "digest",
            ]
        {
            return Err("unsupported decode precedence".into());
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Overlay {
    vocabulary: String,
    block_type: String,
    representation_type: String,
    fields: BTreeMap<Role, String>,
    variants: Variants,
    operations: Operations,
    error_type: String,
    error_constructors: BTreeMap<String, Constructor>,
    payload_helper: Option<Helper>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Variants {
    identity: String,
    compressed: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Operations {
    end: String,
    validate: String,
    encode: String,
    decode: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Constructor {
    variant: String,
    argument: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Helper {
    module: String,
    decode: String,
}

fn identifier(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != "gen"
        && syn::parse_str::<syn::Ident>(name).is_ok()
}

fn distinct_names<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !identifier(value) || !seen.insert(value) {
            return Err(format!("invalid or duplicate Rust identifier {value:?}"));
        }
    }
    Ok(())
}

impl Overlay {
    fn validate(&self, graph: &Graph) -> Result<()> {
        if self.vocabulary != "oddlang-integrity-block-rust-v1" {
            return Err("unsupported integrity-block Rust binding".into());
        }
        if self.fields.len() != 5
            || self.variants.compressed.is_some() != graph.representations.compressed.is_some()
        {
            return Err("Rust binding must cover all field and representation roles".into());
        }
        let names = [
            self.block_type.as_str(),
            &self.representation_type,
            &self.operations.encode,
            &self.operations.decode,
        ];
        distinct_names(
            names.into_iter().chain(
                self.payload_helper
                    .iter()
                    .map(|helper| helper.module.as_str()),
            ),
        )?;
        distinct_names(self.fields.values().map(String::as_str))?;
        distinct_names([self.operations.end.as_str(), &self.operations.validate])?;
        distinct_names(
            std::iter::once(self.variants.identity.as_str())
                .chain(self.variants.compressed.as_deref()),
        )?;
        if let Some(helper) = &self.payload_helper {
            distinct_names([helper.decode.as_str(), "serialize", "deserialize"])?;
        }
        let segments = self
            .error_type
            .strip_prefix("::")
            .unwrap_or(&self.error_type)
            .split("::");
        if segments.enumerate().any(|(index, name)| {
            !identifier(name) && !(index == 0 && matches!(name, "crate" | "super" | "self"))
        }) {
            return Err("invalid Rust error type path".into());
        }
        let ids = graph
            .errors
            .roles()
            .map(|(_, id)| id)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if ids != self.error_constructors.keys().map(String::as_str).collect() {
            return Err("Rust constructors must cover all named errors".into());
        }
        for constructor in self.error_constructors.values() {
            if !identifier(&constructor.variant) {
                return Err("invalid Rust error variant".into());
            }
        }
        Ok(())
    }
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let graph: Graph = read(graph)?;
    graph.validate()?;
    let overlay: Overlay = read(overlay)?;
    overlay.validate(&graph)?;
    let source = render::project(&graph, &overlay);
    let encode = |value| serde_json::to_vec(&value).map_err(|error| error.to_string());
    let domain = serde_json::to_value(&graph).map_err(|error| error.to_string())?;
    let binding = serde_json::to_value(&overlay).map_err(|error| error.to_string())?;
    let cases = serde_json::to_vec_pretty(&obligations::for_graph(&graph))
        .map_err(|error| error.to_string())?;
    let lock = serde_json::json!({
        "format": FORMAT,
        "domainSha256": digest(&encode(domain)?),
        "bindingSha256": digest(&encode(binding)?),
        "artifact": "codec.rs",
        "artifactSha256": digest(source.as_bytes()),
        "obligationsSha256": digest(&cases),
    });
    let lock = serde_json::to_vec_pretty(&lock).map_err(|error| error.to_string())?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("codec.rs"), source).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("projection.lock"), lock).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("obligations.json"), cases).map_err(|error| error.to_string())?;
    if output.symlink_metadata().is_ok() {
        return Err("projection destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|error| error.to_string())
}
