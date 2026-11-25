pub mod avro;
pub mod postgres;
pub mod rust;

use crate::model::{BALANCE, Field, ID, Model, OWNER};
use std::{fs, io, path::Path};

/// Constructor and column order stays stable when a version adds fields.
pub fn ordered_fields(model: &Model) -> Vec<&Field> {
    let mut fields: Vec<_> = model.fields.values().collect();
    fields.sort_by_key(|f| {
        (
            match f.iri.as_str() {
                ID => 0,
                OWNER => 1,
                BALANCE => 2,
                _ => 3,
            },
            &f.iri,
        )
    });
    fields
}
pub fn build(model: &Model, output: &Path) -> io::Result<()> {
    fs::create_dir_all(output)?;
    fs::write(output.join("account.rs"), rust::generate(model))?;
    fs::write(output.join("account.avsc"), avro::generate(model))?;
    fs::write(output.join("schema.sql"), postgres::generate(model))
}
