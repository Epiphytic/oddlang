use super::{Definition, Field, Payload, Schema, Shape};
use serde_json::{Value, json};

pub(super) fn derive(schema: &Schema) -> Value {
    let mut obligations = Vec::new();
    for definition in &schema.types {
        match definition {
            Definition::Record { id, fields } => {
                obligations.push(row(
                    id,
                    "wire-shape",
                    json!({
                        "input":"closed named object or positional tuple",
                        "outputOrder":fields.iter().map(|f| &f.name).collect::<Vec<_>>()
                    }),
                ));
                record_fields(&mut obligations, fields);
            }
            Definition::Adjacent {
                id,
                tag,
                content,
                variants,
            } => {
                obligations.push(row(
                    id,
                    "wire-shape",
                    json!({
                        "tagKey":tag, "contentKey":content,
                        "reject":["unknown tag","duplicate tag or content","unknown object keys"]
                    }),
                ));
                for variant in variants {
                    obligations.push(row(&variant.id, "wire-case", json!({
                        "tag":variant.name,
                        "payload":match &variant.payload {
                            Payload::Unit {} => json!({"emitContent":false}),
                            Payload::Newtype { shape } => json!({"namedContent":shape}),
                            Payload::Record { fields } => json!({
                                "orderedRecord":fields.iter().map(|f| &f.name).collect::<Vec<_>>()
                            }),
                        },
                    })));
                    if let Payload::Record { fields } = &variant.payload {
                        record_fields(&mut obligations, fields);
                    }
                }
            }
            Definition::External { id } => {
                let mut entry = row(id, "external-wire", json!({"contractIdentity":id}));
                entry["qualification"] = json!("separate-contract-required");
                obligations.push(entry);
            }
        }
    }
    for (order, guard) in schema.guards.iter().enumerate() {
        obligations.push(row(
            &guard.id,
            "validation",
            json!({
                "order":order,
                "when":guard.when,
                "predicate":guard.require,
                "owner":guard.owner,
                "variant":guard.variant,
                "firstError":guard.error,
            }),
        ));
    }
    for view in &schema.hash_views {
        obligations.push(row(&view.id, "typed-hash-view", json!(view)));
    }
    for (external, tags) in &schema.hash_tags {
        for (case, tag) in tags {
            obligations.push(row(
                case,
                "hash-tag",
                json!({"external":external, "byte":tag}),
            ));
        }
    }
    json!({"domain":schema.id,"obligations":obligations})
}

fn row(id: &str, role: &str, expected: Value) -> Value {
    json!({
        "declaration":id,
        "role":role,
        "expected":expected,
        "qualification":"independent-witness-required",
    })
}

fn record_fields(rows: &mut Vec<Value>, fields: &[Field]) {
    for (position, field) in fields.iter().enumerate() {
        let optional = matches!(field.shape, Shape::Optional { .. });
        rows.push(row(
            &field.id,
            "wire-field",
            json!({
                "wireName":field.name,
                "position":position,
                "parsePresence":if field.require_presence { "required-value-or-null" } else if optional { "value-or-null-or-omitted" } else { "required" },
                "emitPresence":"always",
                "type":field.shape,
            }),
        ));
    }
}
