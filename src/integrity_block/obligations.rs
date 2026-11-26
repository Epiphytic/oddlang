use super::{Check, Graph};
use serde_json::{Value, json};

pub(super) fn for_graph(graph: &Graph) -> Value {
    let obligations = |construct: &str, operation: &str, witness: Value, expected: Value| {
        json!({
            "declaration": format!("{}:{operation}:{construct}", graph.id),
            "role": operation,
            "witness": witness,
            "expected": expected,
            "qualification": "independent-witness-required",
        })
    };
    let mut rows = Vec::new();
    for (position, field) in graph.fields.iter().enumerate() {
        let role = serde_json::to_value(field.role).expect("serialized field role");
        rows.push(obligations(
            role.as_str().expect("field role is a string"),
            "wire",
            json!({"orderedIndex": position, "wireName": field.wire_name}),
            json!({"required": true, "role": role}),
        ));
    }
    for (position, guard) in graph.validate_order.iter().enumerate() {
        let (invalid, error) = match guard {
            Check::RawSize => (
                json!({"below": graph.bounds.raw_minimum, "above": graph.bounds.raw_maximum}),
                &graph.errors.size,
            ),
            Check::EncodedSize => (
                json!({"below": graph.bounds.encoded_minimum, "above": graph.bounds.encoded_maximum}),
                &graph.errors.size,
            ),
            Check::Offset => (
                json!({"checkedAddition": true, "maximumEnd": graph.bounds.offset_maximum.to_string()}),
                &graph.errors.offset,
            ),
            Check::RepresentationSize => (
                json!({"identity": "encoded length differs from declared raw length", "compressed": "encoded length is not strictly smaller"}),
                &graph.errors.representation,
            ),
        };
        let tag = serde_json::to_value(guard).expect("serialized check");
        rows.push(obligations(
            tag.as_str().expect("check is a string"),
            "validate",
            json!({"checkIndex": position, "invalid": invalid}),
            json!({"error": error}),
        ));
    }
    for (position, guard) in graph.decode_order.iter().enumerate() {
        if graph.representations.compressed.is_none() && guard != "digest" {
            continue;
        }
        let negative = match guard.as_str() {
            "negotiated" => "compressed payload presented under identity negotiation",
            "dictionary" => "zlib member requires a preset dictionary",
            "member" => "truncated or malformed member, extra member or trailing bytes",
            "decoded-size" => "decompression does not produce the declared size",
            "digest" => "SHA-256 of exact decoded bytes differs from claimed digest",
            _ => unreachable!("validated decode guard"),
        };
        rows.push(obligations(
            guard,
            "decode",
            json!({"checkIndexAfterValidation": position, "invalid": negative}),
            json!({"error": graph.errors.representation}),
        ));
    }
    rows.extend([
        obligations("record-shape", "wire", json!({"input": "object or positional tuple"}),
            json!({"accept": "exact required fields", "reject": ["unknown key","duplicate key","missing field","extra tuple element"]})),
        obligations("digest-text", "wire", json!({"input": "wrong length or non-lowercase-hex digest"}),
            json!({"errorMessage": graph.wire.digest_syntax_message})),
        obligations("payload-encoded-bound", "wire", json!({"lengthAbove": graph.bounds.encoded_maximum.div_ceil(3) * 4}),
            json!({"error": graph.errors.size})),
        obligations("payload-syntax", "wire", json!({"input": "malformed or noncanonical standard padded Base64"}),
            json!({"error": graph.errors.payload_syntax})),
        obligations("payload-decoded-bound", "wire", json!({"decodedLengthAbove": graph.bounds.encoded_maximum}),
            json!({"error": graph.errors.payload_decoded_size})),
        obligations("identity", "encode", json!({"input": "bounded bytes and offset under identity negotiation"}),
            json!({"representation": graph.representations.identity, "payload": "unchanged", "digest": graph.digest})),
    ]);
    if let Some(compressed) = &graph.representations.compressed {
        rows.push(obligations("beneficial-compression", "encode",
            json!({"algorithm": compressed.algorithm, "effort": compressed.effort}),
            json!({"chooseCompressedWhen": "negotiated, strictly smaller and encoded size is within bounds",
                   "otherwise": graph.representations.identity})));
    }
    for (role, id) in graph.errors.roles() {
        rows.push(obligations(
            &format!("error-{role}"),
            "named-error",
            json!({"role": role}),
            json!({"id": id, "message": graph.error_messages.get(id)}),
        ));
    }
    json!({"domain": graph.id, "obligations": rows})
}
