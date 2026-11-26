//! Reify compiler-proven checked fragment writers in the portable record vocabulary.
use super::*;
use crate::source_observation::{COMPILER, read, validate_source_files};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    method: String,
    variant: String,
    source_sha256: String,
    evidence_sha256: String,
    boundary: Boundary,
    validation_order: Vec<String>,
    #[serde(default, deserialize_with = "unique_map")]
    length_bounds: BTreeMap<String, u32>,
    #[serde(default, deserialize_with = "unique_map")]
    record_types: BTreeMap<String, String>,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Boundary {
    form: String,
    field: String,
    style: String,
    record: String,
    validator: String,
    writer: String,
    delegate: Option<String>,
    trailing: Vec<serde_json::Value>,
    unproved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    format: String,
    compiler: String,
    method: Option<String>,
    source_sha256: Option<String>,
    source_file: Option<String>,
    #[serde(default, deserialize_with = "unique_map")]
    source_files: BTreeMap<String, String>,
    line: Option<u32>,
    #[serde(default, deserialize_with = "unique_map")]
    cases: BTreeMap<String, Observation>,
    unresolved: Vec<String>,
}

#[derive(Deserialize)]
struct Observation {
    #[serde(flatten)]
    boundary: Boundary,
    slots: Vec<Write>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Write {
    UnsignedBe {
        field: String,
        bits: u8,
    },
    SignedBe {
        field: String,
        bits: u8,
    },
    FixedBytes {
        field: String,
        size: u32,
    },
    EnumU8 {
        field: String,
        path: String,
        #[serde(deserialize_with = "unique_map")]
        tags: BTreeMap<String, u8>,
    },
    Bool {
        field: String,
    },
    Bytes {
        field: String,
        #[serde(rename = "requiresLengthBound")]
        requires_length_bound: bool,
    },
    Optional {
        field: String,
        payload: ObservedPayload,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ObservedPayload {
    UnsignedBe { bits: u8 },
    Record { path: String, members: Vec<Write> },
}

impl Write {
    fn field(&self) -> &str {
        match self {
            Self::UnsignedBe { field, .. }
            | Self::SignedBe { field, .. }
            | Self::FixedBytes { field, .. }
            | Self::EnumU8 { field, .. }
            | Self::Bool { field }
            | Self::Bytes { field, .. }
            | Self::Optional { field, .. } => field,
        }
    }

    fn check(
        &self,
        encoding: &Encoding,
        binding: &Binding,
        records: &BTreeMap<String, Record>,
        record_types: &BTreeMap<String, String>,
    ) -> Result<()> {
        match (self, encoding) {
            (Self::UnsignedBe { bits: observed, .. }, Encoding::UnsignedBe { bits, .. })
            | (Self::SignedBe { bits: observed, .. }, Encoding::SignedBe { bits })
                if observed == bits =>
            {
                Ok(())
            }
            (Self::FixedBytes { size: observed, .. }, Encoding::FixedBytes { size })
                if observed == size =>
            {
                Ok(())
            }
            (Self::Bool { .. }, Encoding::Bool { .. })
            | (
                Self::Bytes {
                    requires_length_bound: true,
                    ..
                },
                Encoding::Bytes { .. },
            ) => Ok(()),
            (
                Self::EnumU8 {
                    path, tags: known, ..
                },
                Encoding::EnumU8 { external, tags, .. },
            ) => {
                let target = binding
                    .externals
                    .get(external)
                    .ok_or("missing native enum type binding")?;
                if &target.path != path
                    || tags.len() != known.len()
                    || !tags.iter().all(|(id, tag)| {
                        target.variants.get(id).and_then(|name| known.get(name)) == Some(tag)
                    })
                {
                    return Err(
                        "observed enum representation differs from its portable catalog".into(),
                    );
                }
                Ok(())
            }
            (
                Self::Optional {
                    payload: ObservedPayload::UnsignedBe { bits: observed },
                    ..
                },
                Encoding::Optional {
                    payload: Payload::UnsignedBe { bits },
                    ..
                },
            ) if observed == bits => Ok(()),
            (
                Self::Optional {
                    payload: ObservedPayload::Record { path, members },
                    ..
                },
                Encoding::Optional {
                    payload: Payload::Record { id },
                    ..
                },
            ) => {
                let name = binding
                    .names
                    .get(id)
                    .ok_or("missing native optional record binding")?;
                if path.rsplit("::").next() != Some(name) || record_types.get(id) != Some(path) {
                    return Err("observed optional record differs from its binding".into());
                }
                let record = records.get(id).ok_or("missing optional record catalog")?;
                if record.fields.len() != members.len() {
                    return Err("observed optional record has different fields".into());
                }
                // The paired record catalog retains its independently qualified
                // member layout. Reconcile it against source before projecting.
                for (native, expected) in members.iter().zip(&record.fields) {
                    let shape = match native {
                        Write::UnsignedBe { bits, .. } => Some((*bits, false)),
                        Write::SignedBe { bits, .. } => Some((*bits, true)),
                        _ => None,
                    };
                    if binding.names.get(&expected.id).map(String::as_str) != Some(native.field())
                        || shape != Some((expected.bits, expected.signed))
                    {
                        return Err("observed optional bit width or ordered fields differ".into());
                    }
                }
                Ok(())
            }
            _ => Err(format!(
                "unsupported or different field encoding: {}",
                self.field()
            )),
        }
    }
}

/// Build a checked fragment layout from compiler observations and a portable field catalog.
pub fn recover(
    root: &Path,
    source: &Path,
    evidence: &Path,
    selection: &Path,
    template: &Path,
    overlay: &Path,
    output: &Path,
) -> Result<()> {
    let selection: Selection = json(selection)?;
    let raw = read(evidence)?;
    let source_hash = digest(&read(source)?);
    if selection.format != "oddlang-frame-fragment-selection-v1"
        || selection.source_sha256 != source_hash
        || selection.evidence_sha256 != digest(&raw)
    {
        return Err("fragment source or observation pins do not match".into());
    }
    let mut evidence: Evidence = serde_json::from_slice(&raw).map_err(|err| err.to_string())?;
    if evidence.format != "oddlang-rust-frame-fragments-v1" || evidence.compiler != COMPILER {
        return Err("unexpected frame fragment compiler or format".into());
    }
    if !evidence.unresolved.is_empty() {
        return Err(format!(
            "unresolved frame fragments: {}",
            evidence.unresolved.join("; ")
        ));
    }
    if evidence.method.as_ref() != Some(&selection.method)
        || evidence.source_sha256.as_ref() != Some(&source_hash)
        || evidence.line.unwrap_or(0) == 0
    {
        return Err("frame fragment observation does not pin the selected method or source".into());
    }
    validate_source_files(
        root,
        source,
        evidence.source_file.as_deref(),
        evidence.source_sha256.as_deref(),
        &evidence.source_files,
    )?;
    let observed = evidence
        .cases
        .remove(&selection.variant)
        .ok_or("missing compiler fragment observation")?;
    if observed.boundary.unproved != ["validator-semantics", "validation-order", "decoder"] {
        return Err("fragment evidence omits independent validation or decode dependencies".into());
    }
    if observed.boundary != selection.boundary {
        return Err("observed checked boundary disagrees with reviewed binding".into());
    }
    let mut graph: Graph = json(template)?;
    let binding: Binding = json(overlay)?;
    if !graph.fragment
        || binding.existing_type.as_ref().map_or_else(
            || observed.boundary.record.rsplit("::").next() != Some(&binding.name),
            |path| path != &observed.boundary.record,
        )
    {
        return Err("selected native binding does not identify the record fragment".into());
    }
    if graph.layout.len() != observed.slots.len()
        || observed.slots.is_empty()
        || !graph.layout.windows(2).all(|pair| pair[0].id < pair[1].id)
    {
        return Err(
            "fragment template needs an unordered field catalog with all observed slots".into(),
        );
    }
    let mut catalog: BTreeMap<_, _> = std::mem::take(&mut graph.layout)
        .into_iter()
        .map(|slot| (slot.id.clone(), slot))
        .collect();
    if !graph.records.keys().eq(selection.record_types.keys()) {
        return Err("optional record paths require exact reviewed native bindings".into());
    }
    for write in observed.slots {
        let mut candidate = catalog.iter().filter(|(id, _)| {
            binding
                .names
                .get(*id)
                .is_some_and(|name| name == write.field())
        });
        let id = candidate
            .next()
            .ok_or("observed slot lacks a portable field identity")?
            .0
            .clone();
        if candidate.next().is_some() {
            return Err("ambiguous native record field".into());
        }
        let slot = catalog
            .remove(&id)
            .ok_or("repeated observed fragment field")?;
        write.check(
            &slot.encoding,
            &binding,
            &graph.records,
            &selection.record_types,
        )?;
        graph.layout.push(slot);
    }
    let length_fields: BTreeMap<_, _> = graph
        .layout
        .iter()
        .filter_map(|slot| {
            if let Encoding::Bytes { maximum, .. } = slot.encoding {
                Some((slot.id.clone(), maximum))
            } else {
                None
            }
        })
        .collect();
    if length_fields != selection.length_bounds {
        return Err(
            "length-prefix narrowing requires exact independently reviewed byte bounds".into(),
        );
    }
    let checked_slots = graph
        .layout
        .iter()
        .filter(|slot| {
            matches!(
                slot.encoding,
                Encoding::UnsignedBe { .. }
                    | Encoding::Bytes { .. }
                    | Encoding::AsciiAtom { .. }
                    | Encoding::Nested { .. }
            )
        })
        .map(|slot| &slot.id);
    if !checked_slots.eq(&selection.validation_order) {
        return Err("recovered layout changes the independently reviewed validation order".into());
    }
    graph.validate(&binding)?;
    let dependencies = encode(&json!({
        "format":"oddlang-frame-fragment-dependencies-v1",
        "fragment":graph.id,
        "variant":selection.variant,
        "validator":observed.boundary.validator,
        "remainingIndependentObligations":observed.boundary.unproved,
        "trailingFrameWrites":observed.boundary.trailing,
        "independentLengthBounds":selection.length_bounds,
        "independentValidationOrder":selection.validation_order,
    }))?;
    let parent = output
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    fs::write(stage.path().join("graph.json"), encode(&graph)?).map_err(|err| err.to_string())?;
    fs::write(stage.path().join("dependencies.json"), dependencies)
        .map_err(|err| err.to_string())?;
    if output.symlink_metadata().is_ok() {
        return Err("frame fragment recovery destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|err| err.to_string())
}
