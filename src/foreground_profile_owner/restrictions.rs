use super::{Deserialize, token};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestrictionLists {
    schema: Schema,
    decision: Decision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Schema {
    format: String,
    id: String,
    source_sha256: String,
    fields: BTreeMap<String, String>,
    shape: serde_json::Value,
    declarations: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    semantics: String,
    version: u32,
    maximum_keys: usize,
    maximum_patterns: usize,
    maximum_pattern_bytes: usize,
    maximum_executable_bytes: usize,
    match_fuel: usize,
    default_allow: String,
}

const ROLES: [&str; 8] = [
    "policy",
    "version",
    "defaults",
    "entries",
    "principal",
    "restrictions",
    "allow",
    "deny",
];

impl RestrictionLists {
    pub(super) fn policy_field(&self) -> &str {
        &self.schema.fields["policy"]
    }

    pub(super) fn validate(&self, config_fields: &[String]) -> Result<(), String> {
        let s = &self.schema;
        let d = &self.decision;
        let shape = serde_json::json!({
            "unknownPolicyFields":"reject","missingDefaults":"emptyRestrictions","missingEntries":"emptyList",
            "missingAllow":"absent","nullAllow":"reject","missingDeny":"emptyList","missingVersion":"reject",
            "missingPrincipal":"reject","missingRestrictions":"reject","versionType":"u32",
            "missingPolicy":"reject","nullPolicy":"reject",
            "sequenceFields":{
                "policy":["version","defaults","entries"],
                "entry":["principal","restrictions"],
                "restrictions":["allow","deny"]
            }
        });
        if s.format != "oddlang-restriction-lists-v1"
            || oxigraph::model::NamedNode::new(&s.id).is_err()
            || s.source_sha256.len() != 64
            || !s
                .source_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || s.shape != shape
            || s.fields.len() != ROLES.len()
            || s.declarations.len() != ROLES.len()
            || ROLES.iter().any(|&r| {
                !s.fields.get(r).is_some_and(|v| token(v))
                    || s.declarations.get(r) != Some(&format!("{}/field/{r}", s.id))
            })
            || config_fields.contains(&s.fields["policy"])
            || [
                &["version", "defaults", "entries"][..],
                &["principal", "restrictions"],
                &["allow", "deny"],
            ]
            .iter()
            .any(|group| {
                group
                    .iter()
                    .map(|r| &s.fields[*r])
                    .collect::<BTreeSet<_>>()
                    .len()
                    != group.len()
            })
            || d.semantics != "additive-absolute-byte-glob-v1"
            || d.version == 0
            || !(1..=4096).contains(&d.maximum_keys)
            || !(1..=4096).contains(&d.maximum_patterns)
            || !(3..=65536).contains(&d.maximum_pattern_bytes)
            || !(1..=65536).contains(&d.maximum_executable_bytes)
            || !(1..=100_000_000).contains(&d.match_fuel)
            || d.default_allow != "/**"
        {
            return Err("invalid command restriction contract".into());
        }
        Ok(())
    }

    pub(super) fn render(&self, out: &mut String) {
        let d = &self.decision;
        writeln!(out, "const POLICY_VERSION: u32 = {};", d.version).unwrap();
        for (name, value) in [
            ("KEYS", d.maximum_keys),
            ("PATTERNS", d.maximum_patterns),
            ("PATTERN_BYTES", d.maximum_pattern_bytes),
            ("EXECUTABLE_BYTES", d.maximum_executable_bytes),
            ("FUEL", d.match_fuel),
        ] {
            writeln!(out, "const POLICY_{name}: usize = {value};").unwrap();
        }
        writeln!(out, "const POLICY_DEFAULT: &str = {:?};", d.default_allow).unwrap();
        let mut parts = include_str!("restriction_lists.rs.in").split("__");
        out.push_str(parts.next().unwrap());
        while let Some(role) = parts.next() {
            write!(out, "{:?}", self.schema.fields[role]).unwrap();
            out.push_str(parts.next().expect("closed field placeholder"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn contract() -> Value {
        let iri = "urn:example:file-selection";
        let fields: BTreeMap<_, _> = ROLES
            .into_iter()
            .map(|r| (r, format!("input_{r}")))
            .collect();
        let declarations: BTreeMap<_, _> = ROLES
            .into_iter()
            .map(|r| (r, format!("{iri}/field/{r}")))
            .collect();
        json!({
            "schema":{
                "format":"oddlang-restriction-lists-v1","id":iri,"sourceSha256":"0".repeat(64),
                "fields":fields,"declarations":declarations,"shape":{
                    "unknownPolicyFields":"reject","missingDefaults":"emptyRestrictions","missingEntries":"emptyList",
                    "missingAllow":"absent","nullAllow":"reject","missingDeny":"emptyList","missingVersion":"reject",
                    "missingPrincipal":"reject","missingRestrictions":"reject","versionType":"u32",
                    "missingPolicy":"reject","nullPolicy":"reject",
                    "sequenceFields":{
                        "policy":["version","defaults","entries"],
                        "entry":["principal","restrictions"],
                        "restrictions":["allow","deny"]
                    }
                }
            },
            "decision":{"semantics":"additive-absolute-byte-glob-v1","version":1,"maximum_keys":5,
                "maximum_patterns":15,"maximum_pattern_bytes":50,"maximum_executable_bytes":64,
                "match_fuel":1000,"default_allow":"/**"}
        })
    }

    #[test]
    fn projects_selected_field_names_and_rejects_ambiguous_or_unsupported_contracts() {
        let mut graph = contract();
        graph["schema"]["fields"]["version"] = json!("__deny__");
        let model: RestrictionLists = serde_json::from_value(graph.clone()).unwrap();
        model.validate(&[]).unwrap();
        let mut rust = String::new();
        model.render(&mut rust);
        assert!(rust.contains("rename = \"__deny__\""));
        assert!(rust.contains("rename = \"input_deny\""));
        assert!(rust.contains("POLICY_KEYS: usize = 5;"));
        assert!(model.validate(&["input_policy".into()]).is_err());

        for (path, value) in [
            ("/schema/fields/deny", json!("input_allow")),
            ("/schema/declarations/allow", json!("urn:wrong")),
            ("/schema/shape/nullAllow", json!("accept")),
            (
                "/schema/shape/sequenceFields/policy",
                json!(["version", "entries", "defaults"]),
            ),
            ("/decision/semantics", json!("unknown")),
        ] {
            let mut invalid = graph.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            let model: RestrictionLists = serde_json::from_value(invalid).unwrap();
            assert!(model.validate(&[]).is_err(), "{path}");
        }
    }
}
