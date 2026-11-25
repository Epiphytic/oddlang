#[allow(dead_code)]
mod support_fixture;
use std::{collections::BTreeSet, fs, path::Path, process::Command};
use support_fixture::*;

fn cli(dir: &Path) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .arg("impact")
        .arg(dir)
        .arg(BALANCE)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn expected(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| format!("urn:oddlang:ledger:{name}"))
        .collect()
}
#[test]
fn reachable_named_dependents_include_indirect_and_nested_imports() {
    let p = fixture();
    assert_eq!(
        cli(p.path()),
        expected(&["Avro", "Rust", "Sql", "TransferFunds"])
    );
    fs::write(p.path().join("extra.ttl"),"<urn:oddlang:ledger:Audit> a <urn:oddlang:core:Projection> ; <urn:oddlang:core:dependsOn> <urn:oddlang:ledger:Sql> .").unwrap();
    assert_eq!(
        cli(p.path()),
        expected(&["Avro", "Rust", "Sql", "TransferFunds"])
    );
    replace(
        p.path(),
        "sql.ttl",
        "\"fields.ttl\"",
        "\"fields.ttl\", \"extra.ttl\"",
    );
    assert_eq!(
        cli(p.path()),
        expected(&["Audit", "Avro", "Rust", "Sql", "TransferFunds"])
    );
}
#[test]
fn toml_reverse_closure_matches_sparql() {
    let data: toml::Table = fs::read_to_string("examples/ledger/comparison/impact.toml")
        .unwrap()
        .parse()
        .unwrap();
    let edges = data["dependencies"].as_table().unwrap();
    let mut reached = BTreeSet::from([BALANCE]);
    loop {
        let before = reached.len();
        for (id, deps) in edges {
            if deps
                .as_array()
                .unwrap()
                .iter()
                .any(|dep| reached.contains(dep.as_str().unwrap()))
            {
                reached.insert(id);
            }
        }
        if before == reached.len() {
            break;
        }
    }
    let named = data["report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        reached.intersection(&named).copied().collect::<Vec<_>>(),
        cli(&v1())
    );
}

#[test]
fn impact_distinguishes_an_unknown_iri_from_a_known_leaf() {
    let model = oddlang::model::load_model(v1()).unwrap();
    assert!(
        oddlang::impact::impact(&model, "urn:oddlang:ledger:Unrelated")
            .unwrap()
            .is_empty()
    );
    let unknown = "urn:oddlang:ledger:balanse";
    let err = oddlang::impact::impact(&model, unknown).unwrap_err();
    assert!(err.contains(unknown), "{err}");
}
