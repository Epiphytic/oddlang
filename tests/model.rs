use oddlang::model::load_model;
mod support_fixture;
use support_fixture::*;

#[test]
fn loads_required_fields_and_overlay_names() {
    let m = load_model(v1()).unwrap();
    assert_eq!(m.fields.len(), 3);
    assert_eq!(m.fields[BALANCE].range, Some((0, i64::MAX)));
    assert_eq!(m.fields[BALANCE].sql_name, "balance");
}
#[test]
fn structural_errors_are_stable_across_equivalent_graphs() {
    for (file, from, to, expected) in [
        ("fields.ttl", "l:owner, ", "", "urn:oddlang:ledger:owner"),
        ("domain.ttl", "fields.ttl", "absent.ttl", "absent.ttl"),
        ("sql.ttl", "\"owner\"", "\"balance\"", "balance"),
        (
            "fields.ttl",
            "9223372036854775807",
            "9223372036854775808",
            "balance/range/max: outside signed 64-bit range",
        ),
        (
            "fields.ttl",
            "o:min 0",
            r#"o:min "abc"^^<http://www.w3.org/2001/XMLSchema#integer>"#,
            "balance/range/min: invalid integer",
        ),
        (
            "operations.ttl",
            "o:dependsOn l:balance",
            "o:dependsOn l:Domain",
            "dependsOn: unresolved urn:oddlang:ledger:Domain",
        ),
        ("fields.ttl", "o:max", "o:unknown", BALANCE),
    ] {
        let p = fixture();
        replace(p.path(), file, from, to);
        let err = load_model(p.path()).unwrap_err();
        assert!(err.contains(expected), "{err}");
        rewrite(p.path());
        assert_eq!(load_model(p.path()).unwrap_err(), err);
    }
}
#[test]
fn equivalent_graphs_produce_the_same_typed_fields() {
    let p = fixture();
    let before = load_model(p.path()).unwrap();
    rewrite(p.path());
    assert_eq!(load_model(p.path()).unwrap().fields, before.fields);
}

#[test]
fn closed_graph_rejects_unrecognized_declarations_and_unowned_terms() {
    for (triple, expected) in [
        (
            "l:Typo a o:Enttiy .",
            "urn:oddlang:ledger:Typo: unknown declaration type",
        ),
        (
            "l:Account a o:Field .",
            "urn:oddlang:ledger:Account: expected exactly one declaration type",
        ),
        (
            "l:Unused o:name \"unowned\" .",
            "urn:oddlang:ledger:Unused: unowned declaration",
        ),
        (
            "l:TransferFunds o:typo l:balance .",
            "urn:oddlang:ledger:TransferFunds: unknown property",
        ),
        (
            "l:Y o:mapping l:Y .",
            "urn:oddlang:ledger:Y: unowned declaration",
        ),
    ] {
        let p = fixture();
        let path = p.path().join("fields.ttl");
        let s = std::fs::read_to_string(&path).unwrap();
        std::fs::write(path, format!("{s}\n{triple}")).unwrap();
        let err = load_model(p.path()).unwrap_err();
        assert!(err.contains(expected), "{err}");
        rewrite(p.path());
        assert_eq!(load_model(p.path()).unwrap_err(), err);
    }
}
