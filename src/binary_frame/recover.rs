//! Match compiler-proven writer operations with stable portable frame identities.
use super::*;
use crate::source_observation::{COMPILER, read, validate_source_files};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    method: String,
    source_sha256: String,
    evidence_sha256: String,
    #[serde(deserialize_with = "unique_map")]
    tuple_slots: BTreeMap<String, Vec<String>>,
    #[serde(deserialize_with = "unique_map")]
    fragment_helpers: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    accepted_unresolved: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    format: String,
    compiler: String,
    method: String,
    source_sha256: Option<String>,
    source_file: Option<String>,
    #[serde(default, deserialize_with = "unique_map")]
    source_files: BTreeMap<String, String>,
    line: Option<u32>,
    maximum: Option<u32>,
    cases: Option<Vec<Observation>>,
    unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    variant: String,
    form: Form,
    tag: u8,
    writes: Option<Vec<Write>>,
    unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Write {
    Unsigned {
        field: String,
        bits: u8,
    },
    Fixed {
        field: String,
        size: u32,
    },
    Enum {
        field: String,
        path: String,
        #[serde(deserialize_with = "unique_map")]
        tags: BTreeMap<String, u8>,
    },
    Fragment {
        field: String,
        path: String,
        helper: String,
    },
    Literal {
        literal: u8,
    },
    Rest {
        field: String,
    },
}

impl Write {
    fn field(&self) -> Option<&str> {
        match self {
            Self::Unsigned { field, .. }
            | Self::Fixed { field, .. }
            | Self::Enum { field, .. }
            | Self::Fragment { field, .. }
            | Self::Rest { field } => Some(field),
            Self::Literal { .. } => None,
        }
    }
}

fn project_enum(
    native: &NativeType,
    tags: &BTreeMap<String, u8>,
    catalog: &BTreeMap<String, u8>,
) -> Result<BTreeMap<String, u8>> {
    if !catalog.keys().eq(native.variants.keys()) || native.variants.len() != tags.len() {
        return Err("observed byte mapping disagrees with the enum catalog".into());
    }
    native
        .variants
        .iter()
        .map(|(id, name)| {
            let tag = tags
                .get(name)
                .ok_or("observed enum case lacks a portable identity")?;
            Ok((id.clone(), *tag))
        })
        .collect()
}

fn find_slot(
    slots: &BTreeMap<String, Slot>,
    binding: &Binding,
    tuple: Option<&[String]>,
    field: Option<&str>,
) -> Result<String> {
    if let (Some(positions), Some(field)) = (tuple, field) {
        let index = field
            .parse::<usize>()
            .map_err(|_| "invalid native tuple position")?;
        return positions
            .get(index)
            .cloned()
            .ok_or("unmapped native tuple position".into());
    }
    let mut matches = slots.iter().filter(|(id, slot)| match field {
        Some(field) => {
            slot.encoding.field() && binding.fields.get(*id).is_some_and(|name| name == field)
        }
        None => matches!(slot.encoding, Encoding::Literal { .. }),
    });
    let (id, _) = matches
        .next()
        .ok_or("observed write lacks a portable slot identity")?;
    if matches.next().is_some() {
        return Err("observed write has an ambiguous slot identity".into());
    }
    Ok(id.clone())
}

struct Projection<'a> {
    selection: &'a Selection,
    binding: &'a Binding,
    types: &'a mut BTreeMap<String, External>,
    written_enums: BTreeSet<String>,
    seen_tuple: BTreeSet<String>,
    seen_fragments: BTreeSet<String>,
    seen_obligations: BTreeSet<String>,
}

impl Projection<'_> {
    fn write(&mut self, write: &Write, slot: &mut Slot) -> Result<()> {
        match (write, &mut slot.encoding) {
            (Write::Unsigned { bits: observed, .. }, Encoding::Unsigned { bits, .. })
                if observed == bits => {}
            (Write::Fixed { size: observed, .. }, Encoding::Fixed { size }) if observed == size => {
            }
            (Write::Rest { .. }, Encoding::Rest { .. }) => {}
            (Write::Literal { literal }, Encoding::Literal { value, .. }) if *value == 0 => {
                *value = *literal
            }
            (Write::Fragment { path, helper, .. }, Encoding::Fragment { id }) => {
                let native = self
                    .binding
                    .types
                    .get(id)
                    .ok_or("missing native fragment binding")?;
                if &native.path != path
                    || self.selection.fragment_helpers.get(id) != Some(helper)
                    || !matches!(self.types.get(id), Some(External::Fragment {}))
                {
                    return Err(format!(
                        "observed fragment differs from its declared boundary: {id}"
                    ));
                }
                self.seen_fragments.insert(id.clone());
            }
            (
                Write::Enum {
                    path,
                    tags: observed,
                    ..
                },
                Encoding::Enum { id, .. },
            ) => {
                let native = self
                    .binding
                    .types
                    .get(id)
                    .ok_or("missing native enum binding")?;
                let Some(External::Enum { tags }) = self.types.get_mut(id) else {
                    return Err("missing portable enum catalog".into());
                };
                if &native.path != path {
                    return Err(format!(
                        "observed enum owner differs from its binding: {id}"
                    ));
                }
                let recovered = project_enum(native, observed, tags)?;
                if self.written_enums.insert(id.clone()) {
                    if tags.values().any(|tag| *tag != 0) {
                        return Err("enum template must leave observed tags vacant".into());
                    }
                    *tags = recovered;
                } else if *tags != recovered {
                    return Err("inconsistent enum byte mappings across frame cases".into());
                }
            }
            _ => {
                return Err(format!(
                    "observed write differs from portable slot type: {}",
                    slot.id
                ));
            }
        }
        Ok(())
    }

    fn case(&mut self, id: &str, case: &mut Case, observed: Observation) -> Result<()> {
        if case.tag != 0 || case.form != observed.form {
            return Err(format!(
                "invalid vacant tag or mismatched variant form: {id}"
            ));
        }
        let writes = observed.writes.ok_or_else(|| {
            format!(
                "unresolved frame payload for {id}: {}",
                observed.unresolved.join("; ")
            )
        })?;
        let guard_observed = observed.unresolved.iter().any(|gap| {
            gap.starts_with("checked guard: ") || gap.starts_with("conditional guard at line ")
        });
        let fragment = case
            .slots
            .iter()
            .any(|slot| matches!(slot.encoding, Encoding::Fragment { .. }));
        if (!case.encode_guards.is_empty() && !guard_observed)
            || (guard_observed && case.encode_guards.is_empty() && !fragment)
        {
            return Err(format!(
                "encoder guard obligations disagree with the template: {id}"
            ));
        }
        if !observed.unresolved.is_empty() {
            self.seen_obligations.insert(id.into());
            if self.selection.accepted_unresolved.get(id) != Some(&observed.unresolved) {
                return Err(format!(
                    "unresolved frame obligations for {id}: {}",
                    observed.unresolved.join("; ")
                ));
            }
        }
        let tuple = self.selection.tuple_slots.get(id).map(Vec::as_slice);
        if (case.form == Form::Tuple) != tuple.is_some() {
            return Err(format!(
                "permit positional slot hints only for tuple cases: {id}"
            ));
        }
        if tuple.is_some() {
            self.seen_tuple.insert(id.into());
        }
        let staged = std::mem::take(&mut case.slots);
        if !staged.windows(2).all(|pair| pair[0].id < pair[1].id) {
            return Err(format!("template slots must form a sorted catalog: {id}"));
        }
        let mut catalog: BTreeMap<_, _> = staged
            .into_iter()
            .map(|slot| (slot.id.clone(), slot))
            .collect();
        let mut next_tuple = 0usize;
        for write in writes {
            let field = write.field();
            if tuple.is_some()
                && let Some(field) = field
            {
                if field.parse::<usize>() != Ok(next_tuple) {
                    return Err("wire order differs from the supported native tuple order".into());
                }
                next_tuple += 1;
            }
            let native = find_slot(&catalog, self.binding, tuple, field)?;
            let mut slot = catalog
                .remove(&native)
                .ok_or("duplicate or unknown observed slot")?;
            self.write(&write, &mut slot)?;
            case.slots.push(slot);
        }
        if !catalog.is_empty() || tuple.is_some_and(|slots| slots.len() != next_tuple) {
            return Err(format!(
                "native payload leaves portable slot identities uncovered: {id}"
            ));
        }
        case.tag = observed.tag;
        Ok(())
    }
}

/// Derive the envelope bounds, variant tags and ordered writes from a pinned observation.
pub fn recover(
    root: &Path,
    source: &Path,
    evidence: &Path,
    selection: &Path,
    template: &Path,
    hints: &Path,
    output: &Path,
) -> Result<()> {
    let selection: Selection = json(selection)?;
    let raw = read(evidence)?;
    let source_hash = digest(&read(source)?);
    if selection.format != "oddlang-tagged-frame-selection-v1"
        || selection.source_sha256 != source_hash
        || selection.evidence_sha256 != digest(&raw)
    {
        return Err("tagged frame source or observation pins do not match".into());
    }
    let evidence: Evidence = serde_json::from_slice(&raw).map_err(|err| err.to_string())?;
    if evidence.format != "oddlang-rust-tagged-frame-v1"
        || evidence.compiler != COMPILER
        || evidence.method != selection.method
    {
        return Err("unexpected tagged frame observer or method".into());
    }
    if !evidence.unresolved.is_empty() {
        return Err(format!(
            "unresolved frame envelope: {}",
            evidence.unresolved.join("; ")
        ));
    }
    if evidence.source_sha256.as_ref() != Some(&source_hash) || evidence.line.unwrap_or(0) == 0 {
        return Err("tagged frame observation does not pin the source file or method".into());
    }
    validate_source_files(
        root,
        source,
        evidence.source_file.as_deref(),
        evidence.source_sha256.as_deref(),
        &evidence.source_files,
    )?;

    let mut graph: Graph = json(template)?;
    let binding: Binding = json(hints)?;
    let case_count = graph.cases.len();
    let cases = evidence
        .cases
        .filter(|cases| !cases.is_empty() && cases.len() == case_count && cases.len() <= 256)
        .ok_or("misaligned portable and observed variants")?;
    let method_parts: Vec<_> = evidence.method.rsplit("::").take(2).collect();
    if graph.maximum != 0 || method_parts.as_slice() != ["encode", binding.name.as_str()] {
        return Err("expected a vacant frame envelope for the selected native encoder".into());
    }
    graph.maximum = evidence
        .maximum
        .ok_or("missing compiler horizon for payload length")?;
    let mut projection = Projection {
        selection: &selection,
        binding: &binding,
        types: &mut graph.types,
        written_enums: BTreeSet::new(),
        seen_tuple: BTreeSet::new(),
        seen_fragments: BTreeSet::new(),
        seen_obligations: BTreeSet::new(),
    };
    let mut seen_variants = BTreeSet::new();
    for observed in cases {
        let mut candidates = binding
            .cases
            .iter()
            .filter(|(_, name)| **name == observed.variant);
        let (id, _) = candidates
            .next()
            .ok_or("observed native variant is unmapped")?;
        if candidates.next().is_some() || !seen_variants.insert(id.clone()) {
            return Err("duplicate observed or bound frame variant".into());
        }
        let case = graph
            .cases
            .get_mut(id)
            .ok_or("observed variant lacks a portable declaration")?;
        projection.case(id, case, observed)?;
    }
    if !projection
        .seen_tuple
        .iter()
        .eq(selection.tuple_slots.keys())
        || !projection
            .seen_fragments
            .iter()
            .eq(selection.fragment_helpers.keys())
        || !projection
            .seen_obligations
            .iter()
            .eq(selection.accepted_unresolved.keys())
    {
        return Err("unused tagged frame positional, fragment or obligation declarations".into());
    }
    graph.validate(&binding)?;
    let obligations = encode(&json!({
        "format": "oddlang-tagged-frame-recovery-obligations-v1",
        "method": evidence.method,
        "role": "binary-frame-encoder",
        "unresolved": selection.accepted_unresolved,
        "additionalQualification": ["decoder", "validation and named errors", "nested fragment interiors"]
    }))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    fs::write(stage.path().join("graph.json"), encode(&graph)?).map_err(|err| err.to_string())?;
    fs::write(stage.path().join("obligations.json"), obligations).map_err(|err| err.to_string())?;
    if output.symlink_metadata().is_ok() {
        return Err("tagged frame destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|err| err.to_string())
}
