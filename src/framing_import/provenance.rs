//! Source witnesses retained independently of the Rust source bytes.
use super::{
    Framing, Result,
    cargo::Build,
    hash, json,
    syntax::{Locations, validate_locations},
    valid_digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SourceMap {
    version: u8,
    source: String,
    file_sha256: String,
    source_bytes: usize,
    build_sha256: String,
    locations: Locations,
    span_sha256: BTreeMap<String, String>,
    facts: BTreeMap<String, Fact>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Fact {
    sites: Vec<String>,
    binding_sha256: String,
}

impl SourceMap {
    fn bind(&self, graph: &Framing) -> Result<BTreeMap<String, Fact>> {
        let mut facts = BTreeMap::new();
        for (triple, sites) in graph.facts() {
            let mut witnesses = Vec::new();
            for name in &sites {
                let coord = self
                    .locations
                    .get(name)
                    .ok_or_else(|| format!("unknown framing coordinate {name}"))?;
                let span = self
                    .span_sha256
                    .get(name)
                    .ok_or_else(|| format!("missing span digest {name}"))?;
                witnesses.push((name, coord, span));
            }
            let digest = hash(&json(&(
                "framing-value-source-v2",
                &self.build_sha256,
                &self.file_sha256,
                &triple,
                &witnesses,
            ))?);
            facts.insert(
                hash(triple.as_bytes()),
                Fact {
                    sites,
                    binding_sha256: digest,
                },
            );
        }
        Ok(facts)
    }
}
pub(super) fn create(
    model: &Framing,
    build: &Build,
    locations: Locations,
    source: &[u8],
) -> Result<Vec<u8>> {
    let mut hashes = BTreeMap::new();
    for (id, coord) in &locations {
        let bytes = source
            .get(coord.byte_start..coord.byte_end)
            .ok_or_else(|| format!("span outside framing source: {id}"))?;
        hashes.insert(id.clone(), hash(bytes));
    }
    let mut map = SourceMap {
        version: 2,
        source: "src/frame.rs".into(),
        file_sha256: hash(source),
        source_bytes: source.len(),
        build_sha256: hash(&json(build)?),
        locations,
        span_sha256: hashes,
        facts: BTreeMap::new(),
    };
    map.facts = map.bind(model)?;
    json(&map)
}
pub(super) fn verify(bytes: &[u8], model: &Framing, build: &Build) -> Result<()> {
    let m: SourceMap =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid framing source map: {e}"))?;
    validate_locations(&m.locations, model)?;
    if m.version != 2
        || m.source != "src/frame.rs"
        || Some(&m.file_sha256) != build.inputs.get(&m.source)
        || m.build_sha256 != hash(&json(build)?)
        || m.locations.keys().ne(m.span_sha256.keys())
        || m.span_sha256.values().any(|s| !valid_digest(s))
        || m.locations.values().any(|c| c.byte_end > m.source_bytes)
        || m.facts != m.bind(model)?
        || json(&m)? != bytes
    {
        return Err("framing graph, build and source-span bindings disagree".into());
    }
    Ok(())
}
