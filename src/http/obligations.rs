//! Rule set 1. Each use of a shared declaration retains its operation and position.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub operation: String,
    pub role: String,
    pub declaration: String,
    pub rule: String,
    pub position: String,
    pub kind: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Obligation {
    pub operation: String,
    pub role: String,
    pub declaration: String,
    pub rule: String,
    pub position: String,
    pub slots: Vec<String>,
}
impl Obligation {
    pub fn witness(&self, kind: &str) -> Slot {
        Slot {
            operation: self.operation.clone(),
            role: self.role.clone(),
            declaration: self.declaration.clone(),
            rule: self.rule.clone(),
            position: self.position.clone(),
            kind: kind.into(),
        }
    }
}
pub type WitnessGap = Slot;

struct Rules<'a> {
    model: &'a HttpContract,
    op: &'a Iri,
    role: &'a str,
    out: &'a mut Vec<Obligation>,
}
impl Rules<'_> {
    fn add(&mut self, id: &Iri, rule: &str, path: &str, kinds: &[&str]) {
        self.out.push(Obligation {
            operation: self.op.to_string(),
            role: self.role.into(),
            declaration: id.to_string(),
            rule: format!("{H}rule:1:{rule}"),
            position: path.into(),
            slots: kinds.iter().map(|s| s.to_string()).collect(),
        });
    }
    fn record(&mut self, id: &Iri, path: &str, inbound: bool) {
        let r = &self.model.records[id];
        let shape = if inbound {
            &["object", "wrong-type"][..]
        } else {
            &["object"][..]
        };
        self.add(id, "record", path, shape);
        for c in &r.constraints {
            self.add(
                c,
                "record",
                path,
                if inbound {
                    &["closed", "unknown-member"]
                } else {
                    &["closed"]
                },
            );
        }
        for fid in &r.fields {
            let f = &self.model.fields[fid];
            let pos = format!(
                "{path}/{}",
                f.wire_name.replace('~', "~0").replace('/', "~1")
            );
            let transfer_out = self.role == "http.provider"
                && self.model.operations[self.op].behavior.is_some()
                && pos.starts_with("response/");
            let source_balance = transfer_out && pos == "response/source/balance";
            let destination_balance = transfer_out && pos == "response/destination/balance";
            let attainable =
                |v: i64| (!source_balance || v < i64::MAX) && (!destination_balance || v > 0);
            let range = f
                .constraints
                .iter()
                .find_map(|c| match self.model.constraints[c] {
                    Constraint::Bounds { min, max } => Some(min..=max),
                    _ => None,
                })
                .unwrap_or(i64::MIN..=i64::MAX);
            for cid in &f.constraints {
                match self.model.constraints[cid] {
                    Constraint::Required => self.add(
                        cid,
                        "required",
                        &pos,
                        if inbound {
                            &["present", "omitted"]
                        } else {
                            &["present"]
                        },
                    ),
                    Constraint::Bounds { min, max } => {
                        let mut kinds = vec![];
                        if attainable(min) {
                            kinds.push("min")
                        } else if destination_balance && max >= 1 {
                            kinds.push("min-attainable")
                        }
                        if attainable(max) {
                            kinds.push("max")
                        } else if source_balance && min < i64::MAX {
                            kinds.push("max-attainable")
                        }
                        if inbound {
                            kinds.push("below")
                        }
                        if inbound {
                            kinds.push("above")
                        }
                        self.add(cid, "bounds", &pos, &kinds)
                    }
                    _ => {}
                }
            }
            let (rule, mut kinds) = match f.shape.as_str() {
                "urn:oddlang:http:Text" => ("text", vec!["unicode"]),
                "urn:oddlang:http:Boolean" => ("boolean", vec!["true", "false"]),
                "urn:oddlang:http:I64" => {
                    let mut ks = vec![];
                    for (kind, n) in [
                        ("zero", 0),
                        ("large", 9007199254740993),
                        ("min", i64::MIN),
                        ("max", i64::MAX),
                    ] {
                        if attainable(n) && range.contains(&n) {
                            ks.push(kind)
                        }
                    }
                    if inbound {
                        ks.extend([
                            "noncanonical",
                            "negative-zero",
                            "leading-plus",
                            "exponent",
                            "whitespace",
                            "overflow",
                            "numeric",
                        ])
                    }
                    ("i64", ks)
                }
                _ => {
                    self.add(
                        fid,
                        "record",
                        &pos,
                        if inbound {
                            &["object", "wrong-type"]
                        } else {
                            &["object"]
                        },
                    );
                    self.record(&f.shape, &pos, inbound);
                    continue;
                }
            };
            if inbound && rule != "i64" {
                kinds.push("wrong-type")
            }
            self.add(fid, rule, &pos, &kinds);
        }
    }
}
pub fn inventory(model: &HttpContract) -> Vec<Obligation> {
    let mut out = vec![];
    for (id, op) in &model.operations {
        for role in &op.roles {
            let provider = role.as_str() == format!("{H}provider");
            let mut r = Rules {
                model,
                op: id,
                role: if provider {
                    "http.provider"
                } else {
                    "http.consumer"
                },
                out: &mut out,
            };
            let mut route = vec!["exact"];
            if op.method == "GET" {
                route.extend(["escaped", "plus", "empty", "dot", "dot-dot"]);
                if provider {
                    route.extend(["bad-percent", "bad-utf8"])
                }
            }
            r.add(id, "route", "request", &route);
            for (pos, inbound) in [("request", provider), ("response", !provider)] {
                if op.method == "GET" && pos == "request" {
                    continue;
                }
                r.add(
                    id,
                    "media",
                    pos,
                    if inbound && pos == "request" {
                        &[
                            "json",
                            "charset",
                            "missing",
                            "unsupported",
                            "wrong-charset",
                            "extra-parameter",
                            "media-before-body",
                        ]
                    } else if inbound {
                        &[
                            "json",
                            "charset",
                            "missing",
                            "unsupported",
                            "wrong-charset",
                            "extra-parameter",
                        ]
                    } else {
                        &["json"]
                    },
                );
                r.add(
                    id,
                    "json",
                    pos,
                    if inbound {
                        &[
                            "valid",
                            "escaped",
                            "empty",
                            "bom",
                            "utf8",
                            "surrogate",
                            "malformed",
                            "duplicate",
                            "trailing",
                        ]
                    } else {
                        &["valid"]
                    },
                );
            }
            r.record(&op.request, "request", provider && op.method != "GET");
            r.record(&op.response, "response", !provider);
            r.add(
                id,
                "response",
                "response",
                if provider {
                    &["success"]
                } else {
                    &["success", "transport", "unexpected-status"]
                },
            );
            for e in &op.errors {
                r.add(
                    e,
                    "error",
                    "response",
                    if provider {
                        &["named"]
                    } else {
                        &["named", "unknown", "malformed"]
                    },
                )
            }
            for obs in &op.observations {
                match model.observations[obs] {
                    Observation::Lookup => {
                        r.add(obs, "lookup", "behavior", &["maximum", "missing"])
                    }
                    Observation::Transfer => r.add(
                        obs,
                        "transfer",
                        "behavior",
                        &[
                            "success",
                            "capacity",
                            "source-missing",
                            "destination-missing",
                            "same-account",
                            "non-positive",
                            "insufficient",
                            "overflow",
                            "missing-both",
                            "destination-and-amount",
                            "same-and-amount",
                            "funds-and-capacity",
                        ],
                    ),
                }
            }
        }
    }
    out.sort_by_key(|o| {
        (
            o.operation.clone(),
            o.role.clone(),
            o.position.clone(),
            o.declaration.clone(),
            o.rule.clone(),
        )
    });
    out
}
pub fn gaps(obligations: &[Obligation], cases: &[Case]) -> Vec<WitnessGap> {
    let covered: BTreeSet<_> = cases
        .iter()
        .flat_map(|c| {
            c.slots
                .iter()
                .filter(|s| s.role == c.role && s.operation == c.operation)
        })
        .collect();
    obligations
        .iter()
        .flat_map(|o| o.slots.iter().map(|k| o.witness(k)))
        .filter(|s| !covered.contains(s))
        .collect()
}
