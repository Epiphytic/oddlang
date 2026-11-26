//! Lower pinned compiler observations through consumer-reviewed field identities.
use super::*;
use std::io::Write as _;

const COMPILER: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    relation: String,
    source_sha256: String,
    evidence_sha256: String,
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
    comparisons: Option<Vec<Comparison>>,
    unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comparison {
    left: Field,
    right: Field,
    #[serde(rename = "type")]
    shape: Shape,
    proof: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    parameter: u8,
    owner: String,
    field: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Shape {
    Bool {},
    Unsigned { bits: u8 },
    FixedBytes { size: u32 },
    Bytes {},
    Named { path: String },
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))
}

fn project_field(
    graph: &Graph,
    binding: &Binding,
    relation: &Relation,
    field: &Field,
    value: &Value,
) -> Result<Operand> {
    let (object, owner) = match (&relation.arguments[0].value, field.parameter) {
        (_, 0) => (RECEIVER, &relation.receiver),
        (Value::Named { id }, 1) => (relation.arguments[0].id.as_str(), id),
        _ => return Err("observed parameter has no record binding".into()),
    };
    if binding.types.get(owner) != Some(&field.owner) {
        return Err(format!(
            "observed owner {} disagrees with record {owner}",
            field.owner
        ));
    }
    let fields = &graph
        .types
        .get(owner)
        .ok_or("absent record binding")?
        .fields;
    let mut matches = fields
        .iter()
        .filter(|(id, _)| binding.fields.get(*id) == Some(&field.field));
    let Some((id, shape)) = matches.next() else {
        return Err(format!(
            "observed field {} has no portable identity",
            field.field
        ));
    };
    if matches.next().is_some() || shape != value {
        return Err(format!(
            "observed field {} has an ambiguous identity or different type",
            field.field
        ));
    }
    Ok(Operand::Field {
        object: object.into(),
        field: id.clone(),
    })
}

fn proven_shape(binding: &Binding, comparison: &Comparison) -> Result<Value> {
    match (&comparison.shape, comparison.proof.as_str()) {
        (Shape::Bool {}, "primitive-equality") => Ok(Value::Bool {}),
        (
            Shape::Unsigned {
                bits: bits @ (8 | 16 | 32 | 64),
            },
            "primitive-equality",
        ) => Ok(Value::Unsigned { bits: *bits }),
        (Shape::FixedBytes { size }, "core-byte-array-equality") if *size > 0 => {
            Ok(Value::FixedBytes { size: *size })
        }
        (Shape::Bytes {}, "alloc-byte-vector-equality") => Ok(Value::Bytes {}),
        (Shape::Named { path }, "local-discriminant-equality") => {
            let mut matches = binding.types.iter().filter(|(_, native)| *native == path);
            let (id, _) = matches
                .next()
                .ok_or("observed enum lacks a portable identity")?;
            if matches.next().is_some() {
                return Err("observed enum has ambiguous portable identity".into());
            }
            Ok(Value::Named { id: id.clone() })
        }
        _ => Err("unsupported comparison proof or type".into()),
    }
}

fn canonical(path: &Path) -> Result<std::path::PathBuf> {
    path.canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", path.display()))
}

fn validate_source_files(root: &Path, source: &Path, evidence: &Evidence) -> Result<()> {
    let root = canonical(root)?;
    let source = canonical(source)?;
    let selected = source.strip_prefix(&root).ok().and_then(Path::to_str);
    if selected != evidence.source_file.as_deref()
        || evidence
            .source_file
            .as_ref()
            .and_then(|file| evidence.source_files.get(file))
            != evidence.source_sha256.as_ref()
        || evidence.source_files.is_empty()
        || evidence.source_files.len() > 4096
    {
        return Err("compiler source inventory does not contain the selected file".into());
    }
    for (name, hash) in &evidence.source_files {
        let relative = Path::new(name);
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!("invalid compiler source path: {name}"));
        }
        let path = canonical(&root.join(relative))?;
        if !path.starts_with(&root) || digest(&read(&path)?) != *hash {
            return Err(format!("compiler source inventory changed: {name}"));
        }
    }
    Ok(())
}

/// Derive one vacant relation from a separately pinned compiler receipt.
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
    let evidence_bytes = read(evidence)?;
    let source_hash = digest(&read(source)?);
    if selection.format != "oddlang-field-conjunction-selection-v1"
        || selection.evidence_sha256 != digest(&evidence_bytes)
        || selection.source_sha256 != source_hash
    {
        return Err("field-conjunction input pins do not match".into());
    }
    let evidence: Evidence =
        serde_json::from_slice(&evidence_bytes).map_err(|err| err.to_string())?;
    if evidence.format != "oddlang-rust-field-conjunction-v1" || evidence.compiler != COMPILER {
        return Err("unexpected field-conjunction compiler or version".into());
    }
    if !evidence.unresolved.is_empty() {
        return Err(format!(
            "unresolved field conjunction: {}",
            evidence.unresolved.join("; ")
        ));
    }
    if evidence.source_sha256.as_ref() != Some(&source_hash) || evidence.line.unwrap_or(0) == 0 {
        return Err("compiler observation does not pin this source declaration".into());
    }

    validate_source_files(root, source, &evidence)?;
    let mut graph: Graph = json(template)?;
    let binding: Binding = json(hints)?;
    let relation = graph
        .relations
        .get(&selection.relation)
        .ok_or("missing selected relation")?;
    let method = binding
        .relations
        .get(&selection.relation)
        .ok_or("missing selected method binding")?;
    let owner = binding
        .types
        .get(&relation.receiver)
        .ok_or("missing method receiver")?;
    if !relation.steps.is_empty()
        || relation.failure.is_some()
        || relation.arguments.len() != 1
        || !matches!(relation.arguments[0].value, Value::Named { .. })
        || format!("{owner}::{}", method.name) != evidence.method
    {
        return Err(
            "selected relation needs a vacant boolean method with one record argument".into(),
        );
    }
    let comparisons = evidence
        .comparisons
        .filter(|items| !items.is_empty() && items.len() <= 256)
        .ok_or("missing or excessive compiler comparisons")?;
    let steps = comparisons
        .iter()
        .map(|comparison| {
            if comparison.left.parameter == comparison.right.parameter {
                return Err("equality needs two distinct record inputs".into());
            }
            let value = proven_shape(&binding, comparison)?;
            Ok(Step::Equal {
                left: project_field(&graph, &binding, relation, &comparison.left, &value)?,
                right: project_field(&graph, &binding, relation, &comparison.right, &value)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    graph
        .relations
        .get_mut(&selection.relation)
        .expect("selected relation")
        .steps = steps;
    graph.validate(&binding)?;
    let encoded = encode(&graph)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|err| format!("cannot create output directory: {err}"))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| format!("cannot stage recovered field relation: {err}"))?;
    staged
        .write_all(&encoded)
        .map_err(|err| format!("cannot stage recovered field relation: {err}"))?;
    staged
        .persist_noclobber(output)
        .map(|_| ())
        .map_err(|err| format!("cannot publish {}: {err}", output.display()))
}
