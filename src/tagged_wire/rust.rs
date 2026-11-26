use super::wire::{Contract, Shape};
use std::fmt::Write;

pub(super) fn render(contract: &Contract) -> String {
    // Compile the validated schema into native data. The evaluator has no dependencies.
    let mut source = include_str!("wire.rs").to_owned();
    source.push_str(
        "\n/// Compiled contract for this wire module.\npub fn schema() -> &'static Contract {\n",
    );
    source.push_str(
        "    static SCHEMA: std::sync::OnceLock<Contract> = std::sync::OnceLock::new();\n",
    );
    source.push_str("    SCHEMA.get_or_init(|| Contract {\n");
    writeln!(source, "        sha256: {:?}.into(),", contract.sha256()).unwrap();
    writeln!(source, "        max_payload: {},", contract.max_payload).unwrap();
    source.push_str("        errors: Errors {\n");
    let errors = &contract.errors;
    for (field, code) in [
        ("short_header", &errors.short_header),
        ("payload_limit", &errors.payload_limit),
        ("length_mismatch", &errors.length_mismatch),
        ("unknown_tag", &errors.unknown_tag),
        ("truncated", &errors.truncated),
        ("trailing", &errors.trailing),
        ("invalid_value", &errors.invalid_value),
    ] {
        writeln!(source, "            {field}: {code:?}.into(),").unwrap();
    }
    source.push_str("        },\n");
    if let Some(stream) = &contract.stream {
        writeln!(source, "        stream: Some(StreamErrors {{ failed: {:?}.into(), incomplete: {:?}.into() }}),", stream.failed, stream.incomplete).unwrap();
    } else {
        source.push_str("        stream: None,\n");
    }
    source.push_str("        variants: BTreeMap::from([\n");
    for (tag, variant) in &contract.variants {
        writeln!(source, "            ({tag}, Variant {{").unwrap();
        writeln!(source, "                name: {:?}.into(),", variant.name).unwrap();
        source.push_str("                fields: vec![\n");
        for field in &variant.fields {
            writeln!(
                source,
                "                    Field {{ name: {:?}.into(), shape: {} }},",
                field.name,
                shape(&field.shape)
            )
            .unwrap();
        }
        source.push_str("                ],\n            }),\n");
    }
    source.push_str("        ]),\n    })\n}\n");
    source
}

fn shape(shape: &Shape) -> String {
    match shape {
        Shape::UInt { width, range } => {
            let range = range.as_ref().map_or_else(
                || "None".to_owned(),
                |range| {
                    format!(
                        "Some(Range {{ max: {}, error: {:?}.into() }})",
                        range.max, range.error
                    )
                },
            );
            format!("Shape::UInt {{ width: {width}, range: {range} }}")
        }
        Shape::FixedBytes { width } => format!("Shape::FixedBytes {{ width: {width} }}"),
        Shape::RestBytes {
            min,
            max,
            error,
            sum,
        } => {
            let sum = sum.as_ref().map_or_else(
                || "None".to_owned(),
                |sum| {
                    format!(
                        "Some(Sum {{ field: {}, max: {}, error: {:?}.into() }})",
                        sum.field, sum.max, sum.error
                    )
                },
            );
            format!(
                "Shape::RestBytes {{ min: {min}, max: {max}, error: {error:?}.into(), sum: {sum} }}"
            )
        }
    }
}
