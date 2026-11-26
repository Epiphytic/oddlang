use super::{Definition, FORMAT, Graph, Result, declare};
use crate::{
    native_package::publish,
    wire_schema::{digest, encode, json},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const RUNTIME: &str = r#"
export type FixedBytesId = keyof typeof definitions;
export type FixedBytes = Readonly<{ copyBytes(): Uint8Array }>;

export class FixedBytesError extends Error {
    readonly id: string;

    constructor(id: string) {
        super(id);
        this.name = "FixedBytesError";
        this.id = id;
    }
}

const nativeLength = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(Uint8Array.prototype), "length"
)!.get!;

export function fromBytes(id: FixedBytesId, input: Uint8Array): FixedBytes {
    if (!Object.hasOwn(definitions, id)) throw new RangeError("unknown fixed-byte identity");
    if (!types.isUint8Array(input)) throw new TypeError("expected a Uint8Array");

    const { length, failure } = definitions[id];
    if (nativeLength.call(input) !== length) throw new FixedBytesError(failure);
    const owned = new Uint8Array(length);
    try {
        owned.set(input);
    } catch (error) {
        if (error instanceof RangeError) throw new FixedBytesError(failure);
        throw error;
    }
    return Object.freeze({ copyBytes: () => new Uint8Array(owned) });
}
"#;

pub fn build(graph: &Path, output: &Path) -> Result<()> {
    let graph: Graph = json(graph)?;
    if graph.format != FORMAT || graph.types.is_empty() {
        return Err("expected a nonempty checked values graph".into());
    }

    let mut identities = BTreeSet::new();
    let mut failures = BTreeSet::new();
    declare(&mut identities, &graph.id)?;
    let mut definitions = BTreeMap::new();
    let mut obligations = Vec::new();
    for (id, definition) in &graph.types {
        declare(&mut identities, id)?;
        let Definition::FixedBytes {
            length,
            failure,
            on_drop,
        } = definition
        else {
            return Err(format!(
                "TypeScript requires a fixed-byte declaration: {id}"
            ));
        };
        failures.insert(failure);
        definitions.insert(id, json!({ "length": length, "failure": failure }));
        obligations.push(json!({
            "declaration": id,
            "role": "checked-value-domain",
            "expected": {
                "length": length,
                "failure": failure,
                "input": "exact-bytes",
                "ownsInputCopy": true,
                "view": "copy",
                "onDrop": on_drop,
            },
            "qualification": "independent-witness-required",
            "disposal": {
                "qualification": "unsupported",
                "reason": "Node.js provides no deterministic destruction or storage erasure for this value.",
            },
        }));
    }
    for failure in failures {
        declare(&mut identities, failure)?;
    }

    let definitions = serde_json::to_string_pretty(&definitions).map_err(|err| err.to_string())?;
    let source = format!(
        "import {{ types }} from \"node:util\";\n\nconst definitions = {definitions} as const;\n{RUNTIME}"
    );
    let obligations = encode(&json!({
        "format": FORMAT,
        "domain": graph.id,
        "target": "typescript",
        "obligations": obligations,
    }))?;
    let lock = encode(&json!({
        "format": "oddlang-checked-values-lock-v1",
        "target": "typescript",
        "domainSha256": digest(&encode(&graph)?),
        "sourceSha256": digest(source.as_bytes()),
        "obligationsSha256": digest(&obligations),
    }))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    for (name, bytes) in [
        ("values.mts", source.as_bytes()),
        ("obligations.json", &obligations),
        ("projection.lock", &lock),
    ] {
        fs::write(stage.path().join(name), bytes).map_err(|err| err.to_string())?;
    }
    publish(stage.path(), output)
}
