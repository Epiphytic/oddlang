use super::wire::{Contract, Shape};
use serde_json::json;
use std::fmt::Write;

pub(super) fn render(contract: &Contract) -> Result<String, String> {
    let mut source = String::from("// Generated from the pinned tagged-wire contract.\n");
    writeln!(
        source,
        "export const CONTRACT_SHA256 = {};\nconst maxPayload = {};",
        json!(contract.sha256()),
        contract.max_payload,
    )
    .unwrap();
    let errors = &contract.errors;
    let errors = json!({
        "shortHeader": errors.short_header,
        "payloadLimit": errors.payload_limit,
        "lengthMismatch": errors.length_mismatch,
        "unknownTag": errors.unknown_tag,
        "truncated": errors.truncated,
        "trailing": errors.trailing,
        "invalidValue": errors.invalid_value,
    });
    writeln!(source, "const errors = {errors};").unwrap();
    source.push_str("const variants: ReadonlyMap<number, Variant> = new Map<number, Variant>([\n");
    for (tag, variant) in &contract.variants {
        writeln!(
            source,
            "  [{tag}, {{ name: {}, fields: [",
            json!(variant.name),
        )
        .unwrap();
        for field in &variant.fields {
            write!(source, "    {{ name: {}, ", json!(field.name)).unwrap();
            match &field.shape {
                Shape::UInt { width, range } => {
                    write!(source, "kind: \"uint\", width: {width}").unwrap();
                    if let Some(range) = range {
                        write!(
                            source,
                            ", range: {{ max: {}n, error: {} }}",
                            range.max,
                            json!(range.error),
                        )
                        .unwrap();
                    }
                }
                Shape::FixedBytes { width } => {
                    write!(source, "kind: \"fixed\", width: {width}").unwrap();
                }
                Shape::RestBytes {
                    min,
                    max,
                    error,
                    sum,
                } => {
                    write!(
                        source,
                        "kind: \"rest\", min: {min}, max: {max}, error: {}",
                        json!(error),
                    )
                    .unwrap();
                    if let Some(sum) = sum {
                        write!(
                            source,
                            ", sum: {{ field: {}, max: {}n, error: {} }}",
                            sum.field,
                            sum.max,
                            json!(sum.error),
                        )
                        .unwrap();
                    }
                }
            }
            source.push_str(" },\n");
        }
        source.push_str("  ] }],\n");
    }
    source.push_str("]);\n");
    source.push_str(include_str!("typescript_binding.in"));
    if let Some(stream) = &contract.stream {
        let errors = json!({"failed": stream.failed, "incomplete": stream.incomplete});
        writeln!(source, "\nconst streamErrors = {errors};").unwrap();
        source.push_str(include_str!("typescript_stream.in"));
    }
    Ok(source)
}
