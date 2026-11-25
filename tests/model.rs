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

#[test]
fn transfer_lowering_matches_reviewed_operand_order() {
    use oddlang::language::Expr;
    use std::fmt::Write;
    fn term(e: &Expr) -> String {
        match e {
            Expr::Argument(id) => id.clone(),
            Expr::Exists(id) => format!("Exists({id})"),
            Expr::Constant(value) => value.to_string(),
            Expr::Read { account, field } => format!("Read({account}, {field})"),
            Expr::Binary(op, l, r) => format!("{op:?}({}, {})", term(l), term(r)),
        }
    }
    let p = fixture();
    let norm = load_model(p.path()).unwrap().operations;
    rewrite(p.path());
    assert_eq!(load_model(p.path()).unwrap().operations, norm);
    let mut out = String::new();
    for (id, op) in norm {
        writeln!(out, "{id}").unwrap();
        for g in op.guards {
            writeln!(
                out,
                "{} {} {} {}",
                g.priority,
                g.iri,
                term(&g.condition),
                g.error
            )
            .unwrap();
        }
        for a in op.assignments {
            writeln!(out, "{} {} <- {}", a.account, a.field, term(&a.value)).unwrap();
        }
    }
    assert_eq!(out, include_str!("fixtures/transfer.lowered"));
}

#[test]
fn invalid_transfer_nodes_have_stable_diagnostics() {
    for (from, to, diagnostic) in [
        ("l:g1", "_:anonymousGuard", "expected named IRI"),
        ("o:priority 1 ; ", "", "expected one priority"),
        ("o:priority 2", "o:priority 1", "duplicate priority"),
        (
            "o:assign _:debit, _:credit",
            "o:assign _:debit, _:credit, [ a o:Assign ; o:account l:source ; o:field l:balance ; o:value l:amount ]",
            "duplicate assignment target",
        ),
        ("o:priority 5", "o:priority 1", "duplicate priority"),
        ("o:priority 1 ;", "o:priority 7 ;", "existence guard"),
        ("a o:Subtract", "a o:Multiply", "unsupported expression"),
        (
            "_:debitValue a o:Subtract ; o:left _:sourceBalance",
            "_:debitValue a o:Subtract ; o:left _:debitValue",
            "cyclic expression",
        ),
        (
            "o:left _:destinationBalance ; o:right l:amount",
            "o:left [ a o:Constant ; o:integer 170141183460469231731687303715884105727 ] ; o:right l:amount",
            "interval exceeds i128",
        ),
        ("o:right _:zero", "o:right l:source", "operand type"),
        (
            "o:integer 0",
            "o:integer 0 ; o:typo 0",
            "/right: unknown property",
        ),
        (
            "o:field l:balance ; o:value _:debitValue",
            "o:field l:missing ; o:value _:debitValue",
            "unknown field",
        ),
        (
            "o:argument l:source, l:destination, l:amount",
            "o:argument l:source, l:destination",
            "arguments",
        ),
    ] {
        let project = fixture();
        replace(project.path(), "transfer.ttl", from, to);
        let error = load_model(project.path()).unwrap_err();
        assert!(error.contains(diagnostic), "{from}: {error}");
        rewrite(project.path());
        replace(project.path(), "transfer.ttl", "_:zero", "_:renamed");
        assert_eq!(load_model(project.path()).unwrap_err(), error);
    }
}
