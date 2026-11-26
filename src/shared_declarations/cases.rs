//! Language-neutral examples with stable declaration IDs and separately bound native order.
use super::*;
use serde_json::{Value as Json, json};

pub(super) fn derive(graph: &Graph, binding: &Binding) -> Result<Json> {
    let values = graph.constants()?;
    let mut cases = BTreeMap::new();
    for constant in &graph.constants {
        cases.insert(
            &constant.id,
            json!({"kind":"constant","unsignedDecimal":values[constant.id.as_str()].to_string()}),
        );
    }
    for constant in &graph.text_constants {
        cases.insert(
            &constant.id,
            json!({"kind":"text-constant","value":constant.value}),
        );
    }
    for alias in &graph.results {
        cases.insert(
            &alias.id,
            json!({"kind":"result","success":"generic","failure":alias.error}),
        );
    }
    for error in &graph.errors {
        let order = &binding.variant_order[&error.id];
        cases.insert(
            &error.id,
            json!({"kind":"error","nativeVariantOrder":order}),
        );
        for variant in &error.variants {
            let samples: &[Option<&str>] = match variant.payload {
                Payload::None => &[None],
                Payload::Text => &[Some(""), Some("sensor {1}\nµ")],
                Payload::U64 => &[Some("0"), Some("18446744073709551615")],
                Payload::Bool => &[Some("false"), Some("true")],
            };
            let mut rows = Vec::new();
            for payload in samples {
                let mut display = String::new();
                for segment in &variant.message {
                    display.push_str(match segment {
                        Segment::Text { text } => text,
                        Segment::Payload { .. } => {
                            payload.expect("validated variant has a payload")
                        }
                    });
                }
                rows.push(json!({"payloadCanonicalText":payload,"display":display}));
            }
            cases.insert(
                &variant.id,
                json!({"kind":"error-variant","payload":variant.payload,"examples":rows}),
            );
        }
    }
    for enumeration in &graph.string_enums {
        let wires: BTreeSet<_> = enumeration
            .variants
            .iter()
            .map(|v| v.wire.as_str())
            .collect();
        let mut unknown = String::new();
        while wires.contains(unknown.as_str()) {
            unknown.push('?');
        }
        cases.insert(&enumeration.id,json!({"kind":"string-enum","nativeVariantOrder":binding.variant_order[&enumeration.id],"rejectWireString":unknown}));
        if let Some(n) = &enumeration.numbers {
            let rejected: Vec<_> = (0u8..=255)
                .filter(|byte| !n.tags.values().any(|tag| tag == byte))
                .collect();
            let (owner, variant) = graph.error_variant(&n.unknown.id)?;
            let argument = n
                .unknown
                .argument
                .as_ref()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_default();
            let display: String = variant
                .message
                .iter()
                .map(|seg| match seg {
                    Segment::Text { text } => text.as_str(),
                    Segment::Payload { .. } => argument.as_str(),
                })
                .collect();
            cases.get_mut(&enumeration.id).unwrap()["numberAdmission"] = json!({"accept":n.tags,"reject":rejected,"error":n.unknown,"errorType":owner.id,"display":display});
        }
        for variant in &enumeration.variants {
            cases.insert(&variant.id,json!({"kind":"string-variant","wireString":variant.wire,"encode":true,"decode":true}));
        }
    }
    Ok(json!({"format":"oddlang-shared-obligations-v1","domain":graph.id,"declarations":cases}))
}
