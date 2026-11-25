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
            "rust.ttl",
            "o:dependsOn l:owner",
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
        (
            "e:SourceMissing",
            "l:Oops",
            "g1/error: invalid guard error IRI urn:oddlang:ledger:Oops",
        ),
        (
            "e:SourceMissing",
            "e:AssignmentConflict",
            "g1/error: reserved commit error urn:oddlang:ledger:error:AssignmentConflict",
        ),
        (
            "e:SourceMissing",
            "e:FieldOutOfRange",
            "g1/error: reserved commit error urn:oddlang:ledger:error:FieldOutOfRange",
        ),
        (
            "o:left l:amount ; o:right _:zero",
            "o:left l:amount, _:zero ; o:right _:zero",
            "g4/condition: expected one left",
        ),
        (
            "o:left l:amount ; o:right _:zero",
            "o:left 1 ; o:right _:zero",
            "g4/condition/left: expected node",
        ),
        (
            "l:g1, l:g2, l:g3, l:g4, l:g5, l:g6",
            "l:g1, l:g2, l:g4, l:g5, l:g6",
            "g3: unowned declaration",
        ),
        (
            "l:g1",
            "_:anonymousGuard",
            "TransferFunds/guard: expected named IRI",
        ),
        ("o:priority 1 ; ", "", "g1: expected one priority"),
        (
            "o:priority 2",
            "o:priority 1",
            "TransferFunds: duplicate priority 1",
        ),
        (
            "o:assign _:debit, _:credit",
            "o:assign _:debit, _:credit, [ a o:Assign ; o:account l:source ; o:field l:balance ; o:value l:amount ]",
            "TransferFunds: duplicate assignment target urn:oddlang:ledger:source urn:oddlang:ledger:balance",
        ),
        (
            "o:priority 5",
            "o:priority 1",
            "TransferFunds: duplicate priority 1",
        ),
        (
            "o:priority 1 ;",
            "o:priority 7 ;",
            "g5/condition/left/account: missing earlier existence guard for urn:oddlang:ledger:source",
        ),
        (
            "a o:Subtract",
            "a o:Multiply",
            "TransferFunds/assign/urn:oddlang:ledger:source/urn:oddlang:ledger:balance/value: unsupported expression urn:oddlang:core:Multiply",
        ),
        (
            "_:debitValue a o:Subtract ; o:left _:sourceBalance",
            "_:debitValue a o:Subtract ; o:left _:debitValue",
            "TransferFunds/assign/urn:oddlang:ledger:source/urn:oddlang:ledger:balance/value/left: cyclic expression",
        ),
        (
            "o:left _:destinationBalance ; o:right l:amount",
            "o:left [ a o:Constant ; o:integer 170141183460469231731687303715884105727 ] ; o:right l:amount",
            "g6/condition/left: interval exceeds i128",
        ),
        (
            "o:right _:zero",
            "o:right l:source",
            "g4/condition: invalid operand type",
        ),
        (
            "o:integer 0",
            "o:integer 0 ; o:typo 0",
            "g4/condition/right: unknown property urn:oddlang:core:typo",
        ),
        (
            "o:field l:balance ; o:value _:debitValue",
            "o:field l:missing ; o:value _:debitValue",
            "TransferFunds/assign/field: unknown field urn:oddlang:ledger:missing",
        ),
        (
            "o:argument l:source, l:destination, l:amount",
            "o:argument l:source, l:destination",
            "TransferFunds: expected source, destination and amount arguments",
        ),
    ] {
        let project = fixture();
        replace(project.path(), "transfer.ttl", from, to);
        let error = load_model(project.path()).unwrap_err();
        assert_eq!(error, format!("urn:oddlang:ledger:{diagnostic}"), "{from}");
        rewrite(project.path());
        replace(project.path(), "transfer.ttl", "_:zero", "_:renamed");
        assert_eq!(load_model(project.path()).unwrap_err(), error);
    }
}

#[test]
fn expression_depth_limit_includes_leaves_and_guard_root() {
    use std::fmt::Write;
    for depth in [24, 25] {
        let p = fixture();
        let path = p.path().join("transfer.ttl");
        let mut ttl = std::fs::read_to_string(&path)
            .unwrap()
            .replace("o:right _:zero", "o:right _:level2");
        for level in 2..depth {
            writeln!(
                ttl,
                "_:level{level} a o:Add ; o:left _:level{} ; o:right _:zero .",
                level + 1
            )
            .unwrap();
        }
        writeln!(ttl, "_:level{depth} a o:Constant ; o:integer 0 .").unwrap();
        std::fs::write(path, ttl).unwrap();
        let model = load_model(p.path());
        if depth == 24 {
            model.unwrap();
        } else {
            let expected = format!(
                "urn:oddlang:ledger:g4/condition/right{}: expression exceeds depth 24",
                "/left".repeat(23)
            );
            assert_eq!(model.unwrap_err(), expected);
            rewrite(p.path());
            assert_eq!(load_model(p.path()).unwrap_err(), expected);
        }
    }
}

#[test]
fn operation_budget_counts_reused_expressions_for_each_guard() {
    use std::fmt::Write;
    for extra_guards in [78, 79] {
        let p = fixture();
        let path = p.path().join("transfer.ttl");
        let mut ttl = std::fs::read_to_string(&path).unwrap().replace(
            "[ a o:Greater ; o:left l:amount ; o:right _:zero ]",
            "_:positive",
        );
        writeln!(
            ttl,
            "_:positive a o:Greater ; o:left l:amount ; o:right _:zero ."
        )
        .unwrap();
        for n in 7..7 + extra_guards {
            writeln!(ttl,"l:TransferFunds o:guard l:g{n} .\nl:g{n} a o:Guard ; o:priority {n} ; o:condition _:positive ; o:error e:NonPositiveAmount .").unwrap();
        }
        std::fs::write(path, ttl).unwrap();
        let model = load_model(p.path());
        if extra_guards == 78 {
            model.unwrap();
        } else {
            let expected = "urn:oddlang:ledger:TransferFunds/assign/urn:oddlang:ledger:source/urn:oddlang:ledger:balance/value: operation exceeds 256 expanded expression nodes";
            assert_eq!(model.unwrap_err(), expected);
            rewrite(p.path());
            assert_eq!(load_model(p.path()).unwrap_err(), expected);
        }
    }
}
