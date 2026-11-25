use super::ordered_fields;
use crate::language::{
    AMOUNT, ASSIGNMENT_CONFLICT, Binary, DESTINATION, Expr, FIELD_OUT_OF_RANGE, Operation, SOURCE,
};
use crate::model::{BALANCE, Model};
use std::fmt::Write;

pub fn generate(model: &Model) -> String {
    let (lo, hi) = model.fields[BALANCE]
        .range
        .expect("typed balance has a range");
    let account = &model.rust_name;
    let fields = ordered_fields(model);
    let mut source = format!(
        r#"#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Balance(i64);

impl TryFrom<i128> for Balance {{
    type Error = &'static str;
    fn try_from(value: i128) -> Result<Self, Self::Error> {{
        if ({lo}_i128..={hi}_i128).contains(&value) {{
            Ok(Self(value as i64))
        }} else {{
            Err("{BALANCE}")
        }}
    }}
}}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct {account} {{
"#
    );
    for field in &fields {
        let ty = if field.range.is_some() {
            "Balance"
        } else {
            "String"
        };
        writeln!(source, "    {}: {ty},", field.rust_name).unwrap();
    }
    writeln!(source, "}}\n\nimpl {account} {{").unwrap();
    let params = fields
        .iter()
        .map(|f| {
            format!(
                "{}: {}",
                f.rust_name,
                if f.range.is_some() { "i128" } else { "String" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        source,
        "    pub fn new({params}) -> Result<Self, &'static str> {{\n        Ok(Self {{"
    )
    .unwrap();
    for f in &fields {
        let n = &f.rust_name;
        if f.range.is_some() {
            writeln!(source, "            {n}: Balance::try_from({n})?,").unwrap()
        } else {
            writeln!(source, "            {n},").unwrap()
        }
    }
    source.push_str("        })\n    }\n");
    for f in fields {
        let n = &f.rust_name;
        let (ty, body) = if f.range.is_some() {
            ("i64", format!("self.{n}.0"))
        } else {
            ("&str", format!("&self.{n}"))
        };
        writeln!(
            source,
            "\n    pub fn {n}(&self) -> {ty} {{\n        {body}\n    }}"
        )
        .unwrap();
    }
    source.push_str("}\n");
    operations(&mut source, model);
    source
}

fn argument(iri: &str) -> &'static str {
    match iri {
        SOURCE => "source",
        DESTINATION => "destination",
        AMOUNT => "i128::from(amount)",
        _ => unreachable!("typed argument"),
    }
}

fn expression(expr: &Expr, model: &Model) -> String {
    match expr {
        Expr::Argument(id) => argument(id).into(),
        Expr::Constant(n) => format!("{n}_i128"),
        Expr::Exists(id) => format!("state.0.contains_key({})", argument(id)),
        Expr::Read { account, field } => {
            let f = &model.fields[field];
            let access = format!("state.0[{}].{}()", argument(account), f.rust_name);
            if f.range.is_some() {
                format!("i128::from({access})")
            } else {
                access
            }
        }
        Expr::Binary(op, left, right) => {
            let op = match op {
                Binary::NotEqual => "!=",
                Binary::Greater => ">",
                Binary::GreaterEqual => ">=",
                Binary::LessEqual => "<=",
                Binary::Add => "+",
                Binary::Subtract => "-",
            };
            format!(
                "({} {op} {})",
                expression(left, model),
                expression(right, model)
            )
        }
    }
}

fn operations(out: &mut String, model: &Model) {
    let account = &model.rust_name;
    let key = &model.fields[&model.key].rust_name;
    writeln!(
        out,
        r#"
pub mod __operations {{
    #[derive(Default)]
    pub struct State(std::collections::BTreeMap<String, super::{account}>);
    impl State {{
        /// Upsert a checked record using its own identity as the map key.
        pub fn insert(&mut self, account: super::{account}) {{
            self.0.insert(account.{key}().to_owned(), account);
        }}
        pub fn rows(&self) -> &std::collections::BTreeMap<String, super::{account}> {{ &self.0 }}
    }}
    pub type Operation = fn(&mut State, &str, &str, i64) -> Result<(), &'static str>;
    pub fn lookup(iri: &str) -> Option<Operation> {{
        match iri {{"#
    )
    .unwrap();
    for (index, id) in model.operations.keys().enumerate() {
        writeln!(out, "            {id:?} => Some(operation_{index}),").unwrap();
    }
    out.push_str("            _ => None,\n        }\n    }\n");
    for (index, op) in model.operations.values().enumerate() {
        emit_operation(out, model, index, op);
    }
    out.push_str("}\n");
}

fn emit_operation(out: &mut String, model: &Model, index: usize, op: &Operation) {
    writeln!(out,"    fn operation_{index}(state: &mut State, source: &str, destination: &str, amount: i64) -> Result<(), &'static str> {{").unwrap();
    for guard in &op.guards {
        writeln!(
            out,
            "        if !({}) {{ return Err({:?}); }}",
            expression(&guard.condition, model),
            guard.error
        )
        .unwrap();
    }
    // Only the two distinct ID bindings can alias. Static duplicate targets were
    // rejected by the loader, so test the single remaining source of collision.
    let mut targeted = std::collections::BTreeSet::new();
    if op.assignments.iter().any(|a| !targeted.insert(&a.field)) {
        writeln!(
            out,
            "        if source == destination {{ return Err({ASSIGNMENT_CONFLICT:?}); }}"
        )
        .unwrap();
    }
    for (i, a) in op.assignments.iter().enumerate() {
        let value = expression(&a.value, model);
        if model.fields[&a.field].range.is_some() {
            writeln!(out,"        let value_{i} = super::Balance::try_from({value}).map_err(|_| {FIELD_OUT_OF_RANGE:?})?;").unwrap();
        } else {
            writeln!(out, "        let value_{i} = ({value}).to_owned();").unwrap();
        }
    }
    for (i, a) in op.assignments.iter().enumerate() {
        writeln!(
            out,
            "        state.0.get_mut({}).expect(\"guarded target\").{} = value_{i};",
            argument(&a.account),
            model.fields[&a.field].rust_name
        )
        .unwrap();
    }
    out.push_str("        Ok(())\n    }\n");
}
