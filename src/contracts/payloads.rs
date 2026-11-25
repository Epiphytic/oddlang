//! Check release wire mappings and portable examples against the resolved ontology.
use crate::model::{BALANCE, Model};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    account_fields: BTreeMap<String, String>,
    required_account_fields: Vec<String>,
    encoding: Value,
    transfer: Option<TransferRules>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferRules {
    operation: String,
    behavior_version: String,
    errors_by_priority: Vec<String>,
    request: BTreeMap<String, String>,
    success: BTreeMap<String, Value>,
    error: BTreeMap<String, String>,
    failure_state: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vectors {
    version: String,
    account: BTreeMap<String, Value>,
    transfer: Option<TransferCases>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferCases {
    behavior_version: String,
    state: Vec<BTreeMap<String, Value>>,
    request: Request,
    error_request: Request,
    success: Success,
    error: Failure,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    source: String,
    destination: String,
    amount: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success {
    ok: bool,
    balances: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    error: String,
}

fn decimal(name: &str, text: &str, min: i64, max: i64) -> Result<(), String> {
    if text
        .parse::<i64>()
        .is_ok_and(|v| (min..=max).contains(&v) && v.to_string() == text)
    {
        Ok(())
    } else {
        Err(format!("{name}: invalid canonical decimal {text}"))
    }
}

pub(super) fn validate(
    version: &str,
    model: &Model,
    wire_bytes: &[u8],
    vector_bytes: &[u8],
) -> Result<(), String> {
    let wire: Wire = serde_json::from_slice(wire_bytes).map_err(|e| format!("wire.json: {e}"))?;
    let vectors: Vectors =
        serde_json::from_slice(vector_bytes).map_err(|e| format!("vectors.json: {e}"))?;
    if vectors.version != version {
        return Err("vectors.json: wrong version".into());
    }
    if !wire.account_fields.keys().eq(model.fields.keys()) {
        return Err("wire.json: accountFields differs from model fields".into());
    }
    let names: BTreeSet<_> = wire.account_fields.values().collect();
    if names.len() != wire.account_fields.len()
        || names.len() != wire.required_account_fields.len()
        || names != wire.required_account_fields.iter().collect()
    {
        return Err("wire.json: requiredAccountFields differs from accountFields".into());
    }
    let (min, max) = model.fields[BALANCE].range.ok_or("balance bounds")?;
    if wire.encoding["balance"]
        != json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$","min":min.to_string(),"max":max.to_string()})
    {
        return Err("wire.json: invalid balance encoding".into());
    }
    let currency = "urn:oddlang:ledger:currencyCode";
    if model.fields.contains_key(currency)
        && wire.encoding["currencyCode"] != json!({"type":"string","minLength":1})
    {
        return Err("wire.json: invalid currencyCode encoding".into());
    }
    let account = |row: &BTreeMap<String, Value>| -> Result<(), String> {
        if row.keys().collect::<BTreeSet<_>>() != names {
            return Err("vectors.json: account fields differ from wire rules".into());
        }
        for (iri, name) in &wire.account_fields {
            let s = row[name]
                .as_str()
                .ok_or_else(|| format!("{name}: expected string"))?;
            if iri == BALANCE {
                decimal(name, s, min, max)?;
            }
            if iri == currency && s.is_empty() {
                return Err(format!("{name}: empty currency"));
            }
        }
        Ok(())
    };
    account(&vectors.account)?;
    match (wire.transfer, vectors.transfer) {
        (None, None) if model.operations.is_empty() => Ok(()),
        (Some(rules), Some(cases)) if model.operations.len() == 1 => {
            let op = model
                .operations
                .get(&rules.operation)
                .ok_or("wire.json: unknown operation")?;
            if !op
                .guards
                .iter()
                .map(|g| &g.error)
                .eq(&rules.errors_by_priority)
            {
                return Err("wire.json: errorsByPriority differs from ontology guards".into());
            }
            if rules.behavior_version != version || cases.behavior_version != version {
                return Err("wrong transfer behaviorVersion".into());
            }
            if serde_json::to_value(rules.request).unwrap()
                != json!({"source":"text","destination":"text","amount":"amount"})
                || wire.encoding["amount"]
                    != json!({"type":"string","pattern":"^(0|-?[1-9][0-9]*)$","min":i64::MIN.to_string(),"max":i64::MAX.to_string()})
            {
                return Err("wire.json: invalid transfer request encoding".into());
            }
            if rules.success.get("ok") != Some(&Value::Bool(true))
                || rules
                    .success
                    .get("balances")
                    .and_then(Value::as_str)
                    .is_none()
                || rules.error.get("error").map(String::as_str) != Some("one of errorsByPriority")
                || rules.failure_state != "unchanged"
            {
                return Err("wire.json: invalid response rules".into());
            }
            for row in &cases.state {
                account(row)?;
            }
            for request in [&cases.request, &cases.error_request] {
                decimal("amount", &request.amount, i64::MIN, i64::MAX)?;
            }
            let ids = BTreeSet::from([&cases.request.source, &cases.request.destination]);
            if !cases.success.ok
                || ids != cases.success.balances.keys().collect()
                || !rules.errors_by_priority.contains(&cases.error.error)
            {
                return Err("vectors.json: invalid transfer response".into());
            }
            for balance in cases.success.balances.values() {
                decimal("balance", balance, min, max)?;
            }
            Ok(())
        }
        _ => Err("wire.json/vectors.json: transfer availability differs from model".into()),
    }
}
