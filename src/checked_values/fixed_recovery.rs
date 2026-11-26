use super::*;
use crate::rust_recovery::shape_model::{
    ArrayExtent, DeclarationShape, Form, Observation, Shape, VOCABULARY,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    source: String,
    domain: String,
    value: String,
    failure: String,
}

/// Recover the extent and storage of a selected type. Its public conversions and failure
/// mapping are a contract supplied by the selector and must be qualified with a caller.
pub fn recover_fixed_bytes(observation: &Path, selection: &Path, output: &Path) -> Result<()> {
    let source = fs::read(observation).map_err(|e| e.to_string())?;
    let observation: Observation = serde_json::from_slice(&source).map_err(|e| e.to_string())?;
    let selected_bytes = fs::read(selection).map_err(|e| e.to_string())?;
    let selection: Selection =
        serde_json::from_slice(&selected_bytes).map_err(|e| e.to_string())?;
    let mut ids = BTreeSet::new();
    for id in [&selection.domain, &selection.value, &selection.failure] {
        declare(&mut ids, id)?;
    }
    if observation.graph.vocabulary != VOCABULARY {
        return Err("fixed-byte recovery requires an array-aware compiler observation".into());
    }
    if observation.compiler != "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
        || selection.source.split("::").nth(1) != Some(observation.crate_name.as_str())
    {
        return Err(
            "observation compiler or crate does not match the pinned adapter selection".into(),
        );
    }
    let mut declarations = observation
        .graph
        .declarations
        .iter()
        .filter(|d| d.id == selection.source);
    let decl = declarations
        .next()
        .ok_or("selected declaration is missing")?;
    if declarations.next().is_some()
        || !decl.parameters.is_empty()
        || !decl.id.starts_with("local::")
    {
        return Err("expected a unique local monomorphic declaration".into());
    }
    let DeclarationShape::Product {
        form: Form::Tuple,
        fields,
    } = &decl.shape
    else {
        return Err("expected a tuple product".into());
    };
    let [field] = fields.as_slice() else {
        return Err("expected one owned storage field".into());
    };
    let Shape::Named {
        id,
        arguments,
        ownership: None,
    } = &field.shape
    else {
        return Err("expected named erasing storage".into());
    };
    let [arg] = arguments.as_slice() else {
        return Err("expected one storage argument".into());
    };
    if field.owner != decl.id
        || field.position != 0
        || field.name.is_some()
        || id != "external::zeroize::Zeroizing::{type}"
        || arg.position != 0
        || arg.parameter != "external::zeroize::Zeroizing::{type}::Z::{type}"
    {
        return Err("unsupported fixed-byte storage binding".into());
    }
    let Shape::Array {
        element,
        extent: ArrayExtent::Fixed { count },
    } = &arg.value
    else {
        return Err("expected a fixed array extent".into());
    };
    if !matches!(**element, Shape::UnsignedInteger { bits: 8 }) {
        return Err("fixed-byte storage requires octets".into());
    }
    let length = u32::try_from(*count).map_err(|_| "extent exceeds portable width")?;
    let definition = Definition::FixedBytes {
        length,
        failure: selection.failure,
        on_drop: DropPolicy::Erase,
    };
    let graph = Graph {
        format: FORMAT.into(),
        id: selection.domain,
        types: BTreeMap::from([(selection.value, definition)]),
    };
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let graph_bytes = encode(&graph)?;
    let receipt = json!({
        "format":"oddlang-fixed-bytes-recovery-v1",
        "observationSha256":digest(&source), "selectionSha256":digest(&selected_bytes),
        "graphSha256":digest(&graph_bytes), "compiler":observation.compiler,
        "crate":observation.crate_name, "declaration":decl.id,
        "observed":{"storageType":id,"element":"octet","length":length},
        "providerAssumption":"onDrop follows from the reviewed Zeroizing provider identity",
        "authored":["domain","value","failure","public method bindings"],
        "requiresIndependentQualification":["provider package identity and drop semantics","public conversions","borrowed view","failure mapping"],
    });
    fs::write(stage.path().join("graph.json"), graph_bytes).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("recovery.lock"), encode(&receipt)?).map_err(|e| e.to_string())?;
    publish(stage.path(), output)
}
