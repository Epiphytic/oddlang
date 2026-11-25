use super::ordered_fields;
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
    source
}
