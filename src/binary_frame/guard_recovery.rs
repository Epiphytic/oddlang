//! Lower proven encoder preflight rules in recorded source order.
use super::*;
use crate::source_observation::{COMPILER, publish, read, validate_source_files};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    method: String,
    source_sha256: String,
    evidence_sha256: String,
    #[serde(deserialize_with = "unique_map")]
    cases: BTreeMap<String, BTreeMap<String, String>>,
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
    cases: Option<Observations>,
    unresolved: Vec<String>,
}

#[derive(Deserialize)]
struct Observations(#[serde(deserialize_with = "unique_map")] BTreeMap<String, Plan>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    form: Form,
    guards: Vec<Rule>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Rule {
    Count {
        field: String,
        minimum: u32,
        maximum: u32,
        error: Error,
    },
    Range {
        field: String,
        maximum: u64,
        error: Error,
    },
    EndAtMost {
        offset: String,
        bytes: String,
        maximum: u64,
        error: Error,
    },
    AtMost {
        left: String,
        right: String,
        error: Error,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Error {
    owner: String,
    variant: String,
}

impl Rule {
    fn project(&self, case: &Case, binding: &Binding) -> Result<(String, Predicate, String)> {
        let (key, predicate, error) = match self {
            Self::Count {
                field,
                minimum,
                maximum,
                error,
            } => {
                let slot = find_slot(case, binding, field)?;
                if !matches!(&slot.encoding, Encoding::Rest { minimum: min, maximum: max, .. }
                    if (minimum, maximum) == (min, max))
                {
                    return Err("source count disagrees with the paired frame slot bounds".into());
                }
                (
                    format!("count:{field}"),
                    Predicate::Count {
                        field: slot.id.clone(),
                    },
                    error,
                )
            }
            Self::Range {
                field,
                maximum,
                error,
            } => {
                let slot = find_slot(case, binding, field)?;
                if !matches!(&slot.encoding, Encoding::Unsigned { bits: 64, maximum: max, .. }
                    if maximum == max)
                {
                    return Err(
                        "source range disagrees with the paired unsigned slot bounds".into(),
                    );
                }
                (
                    format!("range:{field}"),
                    Predicate::Range {
                        field: slot.id.clone(),
                    },
                    error,
                )
            }
            Self::EndAtMost {
                offset,
                bytes,
                maximum,
                error,
            } => {
                let start = find_slot(case, binding, offset)?;
                let body = find_slot(case, binding, bytes)?;
                let predicate = Predicate::EndAtMost {
                    offset: start.id.clone(),
                    bytes: body.id.clone(),
                    maximum: *maximum,
                };
                (format!("end-at-most:{offset}:{bytes}"), predicate, error)
            }
            Self::AtMost { left, right, error } => {
                let first = find_slot(case, binding, left)?;
                let second = find_slot(case, binding, right)?;
                let predicate = Predicate::AtMost {
                    left: first.id.clone(),
                    right: second.id.clone(),
                };
                (format!("at-most:{left}:{right}"), predicate, error)
            }
        };
        if error.owner != binding.error_type {
            return Err("observed guard error owner differs from the native binding".into());
        }
        let mut candidates = binding
            .errors
            .iter()
            .filter(|(_, fault)| fault.variant == error.variant && fault.argument.is_none());
        let (id, _) = candidates
            .next()
            .ok_or("observed guard error lacks a stable identity")?;
        if candidates.next().is_some() {
            return Err("ambiguous guard error identity".into());
        }
        Ok((key, predicate, id.clone()))
    }
}

fn find_slot<'a>(case: &'a Case, binding: &Binding, field: &str) -> Result<&'a Slot> {
    let slots = case.slots.iter().filter(|slot| slot.encoding.field());
    match case.form {
        Form::Tuple => {
            let index = field
                .parse::<usize>()
                .map_err(|_| "invalid observed tuple position")?;
            slots
                .into_iter()
                .nth(index)
                .ok_or_else(|| "unbound frame tuple field".into())
        }
        Form::Named => {
            let mut matches = slots.filter(|slot| {
                binding
                    .fields
                    .get(&slot.id)
                    .is_some_and(|name| name == field)
            });
            let matched = matches.next().ok_or("unbound frame named field")?;
            if matches.next().is_some() {
                return Err("ambiguous frame named field".into());
            }
            Ok(matched)
        }
    }
}

/// Replace vacant encoder preflight lists from a pinned compiler observation.
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
    let bytes = read(evidence)?;
    let source_hash = digest(&read(source)?);
    if selection.format != "oddlang-frame-guard-selection-v1"
        || selection.source_sha256 != source_hash
        || selection.evidence_sha256 != digest(&bytes)
    {
        return Err("frame guard source or observation pins do not match".into());
    }
    let evidence: Evidence = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    if evidence.format != "oddlang-rust-frame-guards-v1" || evidence.compiler != COMPILER {
        return Err("unexpected frame guard compiler or format".into());
    }
    if !evidence.unresolved.is_empty() {
        return Err(format!(
            "unresolved frame guards: {}",
            evidence.unresolved.join("; ")
        ));
    }
    if evidence.method.as_ref() != Some(&selection.method)
        || evidence.source_sha256.as_ref() != Some(&source_hash)
        || evidence.line.unwrap_or(0) == 0
    {
        return Err("frame guard observation does not pin the selected method or source".into());
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
    let method: Vec<_> = selection.method.rsplit("::").take(2).collect();
    if method.as_slice() != ["encode", binding.name.as_str()] {
        return Err("frame guard selection does not identify the bound encoder".into());
    }
    let Observations(observed) = evidence.cases.ok_or("missing compiler guard plans")?;
    if observed.is_empty() || selection.cases.len() != observed.len() {
        return Err("expected exact selected frame guard cases".into());
    }
    for (name, plan) in observed {
        let mut variants = binding.cases.iter().filter(|(_, native)| **native == name);
        let (id, _) = variants.next().ok_or("unbound guard variant")?;
        let cases = selection
            .cases
            .get(id)
            .ok_or("unexpected compiler guard variant")?;
        let case = graph
            .cases
            .get_mut(id)
            .ok_or("missing portable guard variant")?;
        if variants.next().is_some()
            || case.form != plan.form
            || !case.encode_guards.is_empty()
            || plan.guards.is_empty()
            || cases.len() != plan.guards.len()
            || cases.len() > 256
        {
            return Err(format!(
                "guard template must leave a matching vacant preflight: {id}"
            ));
        }
        let mut seen = BTreeSet::new();
        for rule in plan.guards {
            let (key, require, error) = rule.project(case, &binding)?;
            let id = cases
                .get(&key)
                .ok_or("observed guard lacks a stable rule identity")?;
            if !seen.insert(key) {
                return Err("repeated observed guard rule".into());
            }
            case.encode_guards.push(Guard {
                id: id.clone(),
                error,
                require,
            });
        }
    }
    graph.validate(&binding)?;
    publish(output, &encode(&graph)?)
}
