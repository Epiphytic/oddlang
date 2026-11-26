use super::*;
use crate::{
    native_package::publish,
    rust_recovery::shape_model::{
        ArrayExtent, DeclarationShape, Form, Observation, Shape, VOCABULARY,
    },
    wire_schema::{digest, encode, symbol, type_symbol},
};
use serde_json::json;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    declaration: String,
    id: String,
    fields: FieldNames,
    transitions: Transitions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StructuralSelection {
    declaration: String,
    id: String,
    fields: FieldNames,
}

enum Policies {
    Authored(Transitions),
    Observed(Vec<u8>),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldNames {
    pub(super) input: String,
    pub(super) storage: String,
    pub(super) begin: String,
    pub(super) end: String,
}

impl FieldNames {
    pub(super) fn entries(&self) -> [(Role, &str); 4] {
        [
            (Role::Input, &self.input),
            (Role::Storage, &self.storage),
            (Role::Begin, &self.begin),
            (Role::End, &self.end),
        ]
    }
}

fn byte_extent(shape: &Shape) -> Option<u64> {
    let Shape::Named {
        id,
        arguments,
        ownership: None,
    } = shape
    else {
        return None;
    };
    let [item] = arguments.as_slice() else {
        return None;
    };
    if id != "external::zeroize::Zeroizing::{type}"
        || item.name != "Z"
        || item.position != 0
        || item.parameter != "external::zeroize::Zeroizing::{type}::Z::{type}"
    {
        return None;
    }
    let Shape::Array {
        element,
        extent: ArrayExtent::Fixed { count },
    } = &item.value
    else {
        return None;
    };
    matches!(**element, Shape::UnsignedInteger { bits: 8 }).then_some(*count)
}

pub fn recover(observation: &Path, selection: &Path, output: &Path) -> Result<()> {
    let raw = fs::read(selection).map_err(|e| e.to_string())?;
    let selector: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let structure = StructuralSelection {
        declaration: selector.declaration,
        id: selector.id,
        fields: selector.fields,
    };
    recover_selected(
        observation,
        &raw,
        structure,
        Policies::Authored(selector.transitions),
        output,
    )
}

pub fn recover_observed(
    observation: &Path,
    selection: &Path,
    trace: &Path,
    output: &Path,
) -> Result<()> {
    let raw = fs::read(selection).map_err(|e| e.to_string())?;
    let selector = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let trace = fs::read(trace).map_err(|e| e.to_string())?;
    recover_selected(
        observation,
        &raw,
        selector,
        Policies::Observed(trace),
        output,
    )
}

fn recover_selected(
    observation: &Path,
    selection: &[u8],
    selector: StructuralSelection,
    policies: Policies,
    output: &Path,
) -> Result<()> {
    let raw = fs::read(observation).map_err(|e| e.to_string())?;
    let obs: Observation = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if obs.compiler != "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
        || obs.graph.vocabulary != VOCABULARY
        || !selector
            .declaration
            .starts_with(&format!("local::{}::", obs.crate_name))
    {
        return Err("unexpected buffered-input compiler observation or crate".into());
    }
    let mut hits = obs
        .graph
        .declarations
        .iter()
        .filter(|d| d.id == selector.declaration);
    let decl = hits.next().ok_or("buffer declaration missing")?;
    let [param] = decl.parameters.as_slice() else {
        return Err("expected one input type parameter".into());
    };
    if hits.next().is_some()
        || !decl.id.ends_with(&format!("::{}::{{type}}", decl.name))
        || param.kind != "type"
        || param.position != 0
        || param.id != format!("{}::{}::{{type}}", decl.id, param.name)
        || !type_symbol(&decl.name)
        || !symbol(&param.name)
        || decl.name == param.name
        || reserved(&decl.name)
        || reserved(&param.name)
    {
        return Err("invalid buffered-input declaration or parameter".into());
    }
    let DeclarationShape::Product {
        form: Form::Record,
        fields,
    } = &decl.shape
    else {
        return Err("expected a named-field record".into());
    };
    let bindings = selector.fields.entries();
    if bindings.iter().any(|(_, n)| !symbol(n))
        || bindings
            .iter()
            .map(|(_, n)| n)
            .collect::<BTreeSet<_>>()
            .len()
            != 4
    {
        return Err("buffer field aliases must be distinct native field names".into());
    }
    let storage_field = format!("{}::{}", decl.id, selector.fields.storage);
    for gap in &obs.graph.gaps {
        let selected = gap.owner == decl.id
            || gap
                .owner
                .strip_prefix(&decl.id)
                .is_some_and(|rest| rest.starts_with("::"));
        let permitted = (gap.owner == decl.id && gap.reason == "generic-constraints-and-defaults")
            || (gap.owner == storage_field && gap.reason == "external-package-instance");
        if selected && !permitted {
            return Err(format!("unresolved selected buffer shape: {}", gap.reason));
        }
    }
    let mut order = Vec::new();
    let mut capacity = None;
    let mut cursor_width = None;
    for (position, field) in fields.iter().enumerate() {
        let name = field.name.as_deref().ok_or("expected named buffer field")?;
        if field.owner != decl.id
            || field.position != position
            || field.id != format!("{}::{name}", decl.id)
        {
            return Err("inconsistent buffer field owner, position or identity".into());
        }
        let role = bindings
            .iter()
            .find(|(_, n)| *n == name)
            .ok_or("unmapped buffer field")?
            .0;
        match (role, &field.shape) {
            (Role::Input, Shape::Parameter { id }) if id == &param.id => {}
            (Role::Storage, shape) => {
                capacity = Some(byte_extent(shape).ok_or("expected fixed erasing octet storage")?);
            }
            (
                Role::Begin | Role::End,
                Shape::TargetInteger {
                    bits,
                    signed: false,
                },
            ) if matches!(bits, 16 | 32 | 64) => {
                if cursor_width.replace(*bits).is_some_and(|old| old != *bits) {
                    return Err("inconsistent index widths".into());
                }
            }
            _ => return Err(format!("incompatible observed field: {name}")),
        }
        order.push(role);
    }
    let capacity = capacity.ok_or("missing octet storage")?;
    let bits = cursor_width.ok_or("missing cursor width")?;
    if capacity > max_capacity(bits) {
        return Err("extent exceeds observed target layout bound".into());
    }
    let (transitions, trace_sha) = match policies {
        Policies::Authored(transitions) => (transitions, None),
        Policies::Observed(raw) => {
            let trace: trace::Trace = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
            (trace.infer(capacity)?, Some(digest(&raw)))
        }
    };
    let graph = Graph {
        format: FORMAT.into(),
        id: selector.id,
        capacity,
        field_order: order,
        transitions,
    };
    graph.validate()?;
    let bytes = encode(&graph)?;
    let binding = encode(
        &json!({"format":"oddlang-buffered-input-rust-v1","id":graph.id,
        "type":decl.name,"parameter":param.name,"fields":selector.fields}),
    )?;
    let mut receipt = json!({
        "format":"oddlang-buffered-input-recovery-v1","compiler":obs.compiler,
        "observationSha256":digest(&raw),"selectionSha256":digest(selection),"declaration":decl.id,
        "graphSha256":digest(&bytes),"overlaySha256":digest(&binding),"observedIndexBits":bits,
        "observed":["generic input identity","field order","octet storage provider and extent","index representations"],
        "authored":["portable identity","semantic field roles","all transition policies"],
        "requiresIndependentQualification":["implementation behavior","storage release obligation","native visibility","generic constraints and defaults","storage provider package instance","target data layout"],
    });
    if let Some(trace_sha) = trace_sha {
        receipt["format"] = json!("oddlang-observed-buffered-input-recovery-v1");
        receipt["authored"] = json!(["portable identity", "semantic field roles"]);
        receipt["inferred"] = json!([
            "empty-read policy",
            "refill-error policy",
            "consumed-byte policy"
        ]);
        receipt["providerAssumptions"] = json!([
            "fresh exhausted specimens with conforming Read providers",
            "first read writes exactly the declared payload",
            "EOF and injected Interrupted leave storage untouched",
            "EOF provider observes initialized storage before writing",
            "storage erase on drop depends on the locked zeroize provider",
        ]);
        receipt["traceSha256"] = json!(trace_sha);
    }
    let receipt = encode(&receipt)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (file, data) in [
        ("graph.json", bytes),
        ("rust.json", binding),
        ("recovery.lock", receipt),
    ] {
        fs::write(stage.path().join(file), data).map_err(|e| e.to_string())?;
    }
    publish(stage.path(), output)
}
