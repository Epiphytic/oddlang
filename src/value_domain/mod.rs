//! Language-neutral nullable unsigned value domains. Host API names live in a separate binding.
use crate::model::{allowed, core, instances, iri, literal, load_graph, named, one};
use oxigraph::model::{
    Graph, NamedOrBlankNodeRef,
    vocab::{rdf, xsd},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
mod rust;
mod typescript;

#[derive(Serialize)]
pub struct Domain {
    id: String,
    bits: u8,
    minimum: u64,
    maximum: u64,
    default_value: u64,
    range_error: String,
    errors: BTreeMap<String, String>,
}

fn integer(g: &Graph, id: &str, prop: &str) -> Result<u64> {
    let value = literal(one(g, &named(id), prop, id)?, xsd::INTEGER, id)?;
    value
        .parse()
        .map_err(|_| format!("{id}: {prop} must be an unsigned integer"))
}

fn load(root: &Path) -> Result<Domain> {
    let (g, _) = load_graph(root)?;
    let domains = instances(&g, "OptionalUnsignedRange")?;
    let [id] = domains
        .into_iter()
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "expected exactly one OptionalUnsignedRange")?;
    let errors = instances(&g, "NamedError")?;
    let mut messages = BTreeMap::new();
    for triple in g.iter() {
        let NamedOrBlankNodeRef::NamedNode(subject) = triple.subject else {
            return Err("value domain requires named declarations".into());
        };
        let s = subject.as_str();
        let ty = if s == id {
            "OptionalUnsignedRange"
        } else if errors.contains(s) {
            "NamedError"
        } else {
            return Err(format!("undeclared subject {s}"));
        };
        if triple.predicate == rdf::TYPE && triple.object != core(ty).as_ref().into() {
            return Err(format!("{s}: unexpected declaration type"));
        }
    }
    for error in errors {
        allowed(&g, &named(&error), &["message"], &error)?;
        let message = literal(
            one(&g, &named(&error), "message", &error)?,
            xsd::STRING,
            &error,
        )?;
        messages.insert(error, message);
    }
    allowed(
        &g,
        &named(&id),
        &["bits", "minimum", "maximum", "defaultValue", "rangeError"],
        &id,
    )?;
    let bits = u8::try_from(integer(&g, &id, "bits")?).map_err(|_| "invalid bit width")?;
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(format!("{id}: unsupported unsigned width"));
    }
    let minimum = integer(&g, &id, "minimum")?;
    let maximum = integer(&g, &id, "maximum")?;
    let default_value = integer(&g, &id, "defaultValue")?;
    if maximum > (u64::MAX >> (64 - bits)) || !(minimum..=maximum).contains(&default_value) {
        return Err(format!("{id}: inconsistent interval, width or default"));
    }
    let range_error = iri(one(&g, &named(&id), "rangeError", &id)?, &id)?;
    if !messages.contains_key(&range_error) {
        return Err(format!("{id}: unresolved range error {range_error}"));
    }
    Ok(Domain {
        id,
        bits,
        minimum,
        maximum,
        default_value,
        range_error,
        errors: messages,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    domain: String,
    type_name: String,
    constructor: String,
    accessor: String,
    maximum_constant: String,
    error_type: String,
    variants: BTreeMap<String, String>,
}

fn check_names(names: &[&str]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !name.bytes().next().is_some_and(|v| v.is_ascii_alphabetic())
            || !name.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_')
            || syn::parse_str::<syn::Ident>(name).is_err()
            || !seen.insert(name)
        {
            return Err(format!("invalid or duplicate Rust API name {name}"));
        }
    }
    Ok(())
}

impl Domain {
    fn cases(&self) -> serde_json::Value {
        use serde_json::json;
        let mut values = BTreeSet::from([
            0,
            self.minimum,
            self.maximum,
            self.default_value,
            u64::MAX >> (64 - self.bits),
        ]);
        values.extend(self.minimum.checked_sub(1));
        values.extend(
            self.maximum
                .checked_add(1)
                .filter(|v| *v <= u64::MAX >> (64 - self.bits)),
        );
        let mut cases = vec![
            json!({"rule":"absent","input":null,"value":null}),
            json!({"rule":"default","value":self.default_value.to_string()}),
        ];
        for v in values {
            let n = v.to_string();
            cases.push(if (self.minimum..=self.maximum).contains(&v) {
                json!({"rule":"inRange","input":n,"value":n})
            } else {
                json!({"rule":"outOfRange","input":n,"error":self.range_error})
            });
        }
        json!({"domain":self.id,"cases":cases,"errorMessages":self.errors})
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn build(root: &Path, binding_path: &Path, output: &Path) -> Result<()> {
    let domain = load(root)?;
    let binding: Binding =
        serde_json::from_slice(&fs::read(binding_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if binding.domain != domain.id || !binding.variants.keys().eq(domain.errors.keys()) {
        return Err("binding must name every semantic declaration exactly".into());
    }
    check_names(&[&binding.type_name, &binding.error_type])?;
    check_names(&[
        &binding.constructor,
        &binding.accessor,
        &binding.maximum_constant,
    ])?;
    check_names(
        &binding
            .variants
            .values()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    let source = rust::render(&domain, &binding);
    let binding = serde_json::to_vec(&binding).map_err(|e| e.to_string())?;
    publish(&domain, "value.rs", source, Some(digest(&binding)), output)
}

pub fn build_typescript(root: &Path, output: &Path) -> Result<()> {
    let domain = load(root)?;
    publish(
        &domain,
        "value.mts",
        typescript::render(&domain),
        None,
        output,
    )
}

fn publish(
    domain: &Domain,
    file: &str,
    source: String,
    binding: Option<String>,
    output: &Path,
) -> Result<()> {
    let canonical = serde_json::to_vec(domain).map_err(|e| e.to_string())?;
    let cases = serde_json::to_vec_pretty(&domain.cases()).map_err(|e| e.to_string())?;
    let lock = serde_json::to_vec_pretty(&serde_json::json!({
        "format": "oddlang-value-domain-v1", "domainSha256": digest(&canonical),
        "bindingSha256": binding, "obligationsSha256": digest(&cases),
        "artifact": file, "artifactSha256": digest(source.as_bytes())
    }))
    .map_err(|e| e.to_string())?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, data) in [
        (file, source.as_bytes()),
        ("obligations.json", &cases),
        ("projection.lock", &lock),
    ] {
        fs::write(tmp.path().join(name), data).map_err(|e| e.to_string())?;
    }
    if output.symlink_metadata().is_ok() {
        return Err("output already exists".into());
    }
    fs::rename(tmp.path(), output).map_err(|e| e.to_string())
}
