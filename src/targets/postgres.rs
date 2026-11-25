use super::ordered_fields;
use crate::model::Model;
pub fn generate(model: &Model) -> String {
    let columns = ordered_fields(model)
        .into_iter()
        .map(|field| {
            let name = format!("\"{}\"", field.sql_name);
            let mut column = match field.range {
                Some((lo, hi)) => {
                    format!("{name} BIGINT NOT NULL CHECK ({name} BETWEEN {lo} AND {hi})")
                }
                None => format!("{name} TEXT NOT NULL"),
            };
            if field.iri == model.key {
                column.push_str(" PRIMARY KEY")
            }
            column
        })
        .collect::<Vec<_>>()
        .join(",\n    ");
    format!("CREATE TABLE \"{}\" (\n    {columns}\n);\n", model.sql_name)
}
