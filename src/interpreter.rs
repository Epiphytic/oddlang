//! Ledger-core evaluation over a checked entry state.
use crate::{
    language::{
        AMOUNT, ASSIGNMENT_CONFLICT, Binary, DESTINATION, Expr, FIELD_OUT_OF_RANGE, SOURCE,
    },
    model::Model,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldValue {
    Text(String),
    Integer(i64),
}
pub type Row = BTreeMap<String, FieldValue>;

pub struct State<'m> {
    model: &'m Model,
    rows: BTreeMap<String, Row>,
}

impl<'m> State<'m> {
    pub fn new(model: &'m Model) -> Self {
        Self {
            model,
            rows: BTreeMap::new(),
        }
    }

    pub fn rows(&self) -> &BTreeMap<String, Row> {
        &self.rows
    }

    /// Build the map key from the validated record's own identity.
    pub fn insert(&mut self, row: Row) -> Result<(), String> {
        for (iri, field) in &self.model.fields {
            match (row.get(iri), field.range) {
                (Some(FieldValue::Text(_)), None) => (),
                (Some(FieldValue::Integer(n)), Some((lo, hi))) if (lo..=hi).contains(n) => (),
                _ => return Err(format!("invalid field {iri}")),
            }
        }
        if row.len() != self.model.fields.len() {
            return Err("unknown field".into());
        }
        let FieldValue::Text(key) = &row[&self.model.key] else {
            unreachable!("validated text key")
        };
        self.rows.insert(key.clone(), row);
        Ok(())
    }

    pub fn execute(
        &mut self,
        operation: &str,
        source: &str,
        destination: &str,
        amount: i64,
    ) -> Result<(), String> {
        let op = self
            .model
            .operations
            .get(operation)
            .ok_or_else(|| format!("unknown operation {operation}"))?;
        let entry = Entry {
            rows: &self.rows,
            source,
            destination,
            amount,
        };
        for guard in &op.guards {
            if let Value::Boolean(false) = entry.evaluate(&guard.condition) {
                return Err(guard.error.clone());
            }
        }
        let mut seen = BTreeSet::new();
        let mut pending = Vec::new();
        for assign in &op.assignments {
            let id = entry.id(&assign.account);
            if !seen.insert((id, &assign.field)) {
                return Err(ASSIGNMENT_CONFLICT.into());
            }
            pending.push((id, &assign.field, entry.evaluate(&assign.value)));
        }
        let mut checked = Vec::new();
        for (id, field, value) in pending {
            let value = match (value, self.model.fields[field].range) {
                (Value::Integer(n), Some((lo, hi))) => {
                    if !(i128::from(lo)..=i128::from(hi)).contains(&n) {
                        return Err(FIELD_OUT_OF_RANGE.into());
                    }
                    FieldValue::Integer(n as i64)
                }
                (Value::Text(s), None) => FieldValue::Text(s),
                _ => unreachable!("typed assignment"),
            };
            checked.push((id, field, value));
        }
        for (id, field, value) in checked {
            *self
                .rows
                .get_mut(id)
                .expect("guarded account")
                .get_mut(field)
                .expect("declared field") = value;
        }
        Ok(())
    }
}

enum Value {
    Text(String),
    Integer(i128),
    Boolean(bool),
}

impl Value {
    fn integer(self) -> i128 {
        if let Self::Integer(n) = self {
            n
        } else {
            unreachable!("typed integer operand")
        }
    }
}

struct Entry<'a, 'r> {
    rows: &'r BTreeMap<String, Row>,
    source: &'a str,
    destination: &'a str,
    amount: i64,
}

impl<'a> Entry<'a, '_> {
    fn id(&self, argument: &str) -> &'a str {
        match argument {
            SOURCE => self.source,
            DESTINATION => self.destination,
            _ => unreachable!("bound ID"),
        }
    }

    fn evaluate(&self, expr: &Expr) -> Value {
        match expr {
            Expr::Argument(arg) if arg == AMOUNT => Value::Integer(self.amount.into()),
            Expr::Argument(arg) => Value::Text(self.id(arg).into()),
            Expr::Exists(arg) => Value::Boolean(self.rows.contains_key(self.id(arg))),
            Expr::Read { account, field } => match &self.rows[self.id(account)][field] {
                FieldValue::Integer(n) => Value::Integer((*n).into()),
                FieldValue::Text(s) => Value::Text(s.clone()),
            },
            Expr::Constant(n) => Value::Integer(*n),
            Expr::Binary(op, left, right) => {
                let left = self.evaluate(left);
                let right = self.evaluate(right);
                match op {
                    Binary::NotEqual => {
                        let (Value::Text(a), Value::Text(b)) = (left, right) else {
                            unreachable!("typed inequality")
                        };
                        Value::Boolean(a != b)
                    }
                    Binary::Greater => Value::Boolean(left.integer() > right.integer()),
                    Binary::GreaterEqual => Value::Boolean(left.integer() >= right.integer()),
                    Binary::LessEqual => Value::Boolean(left.integer() <= right.integer()),
                    Binary::Add => Value::Integer(left.integer() + right.integer()),
                    Binary::Subtract => Value::Integer(left.integer() - right.integer()),
                }
            }
        }
    }
}
