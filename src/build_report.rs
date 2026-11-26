//! Project an ordered JSON report of immutable release facts.
use crate::local_command_client::{Callback, Guard};
use crate::wire_schema::{digest, encode, iri, symbol};
use serde::{Deserialize, Serialize};
mod recover;
pub use recover::recover;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

pub(crate) const TARGET_SCRIPT: &str = r#"fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").expect("Cargo target");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output"));
    std::fs::write(out.join("oddlang-build-target.rs"), format!("{target:?}")).expect("write build target");
}
"#;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format: String,
    id: String,
    fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guards: Vec<Guard>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    id: String,
    wire: String,
    source: Source,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Source {
    Unsigned {
        bits: u8,
        value: u64,
    },
    UnsignedList {
        bits: u8,
        length: usize,
        value: Vec<u64>,
    },
    UnsignedReferenceList {
        bits: u8,
        length: usize,
        items: Vec<Reference>,
    },
    PackageVersion,
    BuildTarget,
    Revision {
        value: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    chain: Vec<IntegerDeclaration>,
    conversion: Conversion,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IntegerDeclaration {
    id: String,
    integer: Integer,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Literal>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(untagged)]
enum Literal {
    Signed(i64),
    Unsigned(u64),
}

impl Literal {
    fn integer(self) -> i128 {
        match self {
            Self::Signed(value) => value.into(),
            Self::Unsigned(value) => value.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Integer {
    signed: bool,
    bits: u8,
}

impl Integer {
    fn valid(self) -> bool {
        matches!(self.bits, 8 | 16 | 32 | 64)
    }
    fn contains(self, value: i128) -> bool {
        if !self.valid() {
            return false;
        }
        let limit = 1_i128 << (self.bits - u8::from(self.signed));
        let min = if self.signed { -limit } else { 0 };
        (min..limit).contains(&value)
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Conversion {
    Identity,
    Widen,
    Wrap,
}

fn resolved(bits: u8, length: usize, items: &[Reference]) -> Result<Vec<u64>, String> {
    let dest = Integer {
        signed: false,
        bits,
    };
    if !dest.valid() || items.len() != length {
        return Err("invalid reference-list length or integer width".into());
    }
    items
        .iter()
        .map(|item| {
            let (last, aliases) = item
                .chain
                .split_last()
                .ok_or("empty integer declaration chain")?;
            let integer = last.integer;
            let value = last
                .value
                .ok_or("reference chain must end at its literal declaration")?
                .integer();
            let mut ids = BTreeSet::new();
            for declaration in &item.chain {
                iri(&declaration.id)?;
                if !ids.insert(&declaration.id) || declaration.integer != integer {
                    return Err("cyclic or type-changing integer declaration chain".into());
                }
            }
            if !integer.contains(value) || aliases.iter().any(|row| row.value.is_some()) {
                return Err("invalid literal value or intermediate declaration".into());
            }
            match item.conversion {
                Conversion::Identity if integer == dest => Ok(value as u64),
                Conversion::Widen if !integer.signed && integer.bits <= bits => Ok(value as u64),
                Conversion::Wrap => Ok(value.rem_euclid(1_i128 << bits) as u64),
                _ => Err("conversion disagrees with source and destination integer types".into()),
            }
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    entry: String,
}

fn load(graph: &[u8], overlay: &[u8]) -> Result<(Report, Binding), String> {
    let report: Report = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    if report.format != "oddlang-build-report-v1"
        || report.fields.is_empty()
        || !symbol(&binding.entry)
    {
        return Err("invalid build report format, fields or entry binding".into());
    }
    iri(&report.id)?;
    let mut ids = BTreeSet::from([&report.id]);
    let mut names = BTreeSet::new();
    for guard in &report.guards {
        guard.validate()?;
    }
    for field in &report.fields {
        iri(&field.id)?;
        if !ids.insert(&field.id) || field.wire.is_empty() || !names.insert(&field.wire) {
            return Err(format!(
                "duplicate identity, empty or duplicate build report wire name: {}",
                field.id
            ));
        }
        let fits =
            |bits: u8, v: u64| matches!(bits, 8 | 16 | 32 | 64) && v <= (u64::MAX >> (64 - bits));
        match &field.source {
            Source::Unsigned { bits, value } if !fits(*bits, *value) => {
                return Err(format!(
                    "invalid unsigned field width or range: {}",
                    field.id
                ));
            }
            Source::UnsignedList {
                bits,
                length,
                value,
            } if !fits(*bits, 0)
                || *length != value.len()
                || !value.iter().all(|v| fits(*bits, *v)) =>
            {
                return Err(format!(
                    "invalid unsigned list width, length or range: {}",
                    field.id
                ));
            }
            Source::UnsignedReferenceList {
                bits,
                length,
                items,
            } => {
                resolved(*bits, *length, items)?;
            }
            _ => {}
        }
        if let Source::Revision { value } = &field.source {
            if value.len() != 40
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(format!("expected full lowercase revision: {}", field.id));
            }
        }
    }
    let mut declarations = BTreeMap::new();
    for field in &report.fields {
        if let Source::UnsignedReferenceList { items, .. } = &field.source {
            for item in items {
                for (index, declaration) in item.chain.iter().enumerate() {
                    let next = item.chain.get(index + 1).map(|decl| decl.id.as_str());
                    let meaning = (
                        declaration.integer,
                        declaration.value.map(Literal::integer),
                        next,
                    );
                    if ids.contains(&declaration.id)
                        || declarations
                            .insert(&declaration.id, meaning)
                            .is_some_and(|prior| prior != meaning)
                    {
                        return Err(format!(
                            "conflicting build report declaration: {}",
                            declaration.id
                        ));
                    }
                }
            }
        }
    }
    Ok((report, binding))
}

pub(crate) fn callback(graph: &[u8], overlay: &[u8]) -> Result<Callback, String> {
    let (report, binding) = load(graph, overlay)?;
    Ok(Callback {
        operation: Some(report.id),
        entry: binding.entry,
        arguments: report.guards.iter().map(Guard::requirement).collect(),
    })
}

pub(crate) fn build(graph: &[u8], overlay: &[u8], out: &Path) -> Result<(), String> {
    let (report, binding) = load(graph, overlay)?;
    let mut source = String::new();
    writeln!(
        source,
        "pub fn {}(matches: ::clap::ArgMatches) -> ::std::process::ExitCode {{",
        binding.entry
    )
    .unwrap();
    if !report.guards.is_empty() {
        if report
            .guards
            .iter()
            .any(|guard| matches!(guard, Guard::PathPresent { .. }))
        {
            source.push_str("    use ::std::path::PathBuf;\n");
        }
        source.push_str("    const BINDING: () = ();\n");
        for guard in &report.guards {
            let (condition, error) = guard.rust_rejection();
            writeln!(
                source,
                "    match (|| -> Result<bool, ()> {{ Ok({condition}) }})() {{
        Ok(true) => {{ eprintln!(\"{{}}\", {:?}); return ::std::process::ExitCode::from({}); }}
        Err(()) => return ::std::process::ExitCode::FAILURE,
        Ok(false) => (),
    }}",
                error.stderr, error.exit
            )
            .unwrap();
        }
    } else {
        source.push_str(
            "    let _ = matches;
",
        );
    }
    source.push_str(
        "    let mut line = String::from(\"{\");
",
    );
    let mut obligations = Vec::new();
    for (index, field) in report.fields.iter().enumerate() {
        let mut prefix = if index == 0 {
            String::new()
        } else {
            ",".to_owned()
        };
        prefix.push_str(&serde_json::to_string(&field.wire).map_err(|e| e.to_string())?);
        prefix.push(':');
        let dynamic = match &field.source {
            Source::BuildTarget => {
                Some(r#"include!(concat!(env!("OUT_DIR"), "/oddlang-build-target.rs"))"#)
            }
            Source::PackageVersion => Some(r#"env!("CARGO_PKG_VERSION")"#),
            Source::Unsigned { value, .. } => {
                prefix.push_str(&value.to_string());
                None
            }
            Source::UnsignedList { value, .. } => {
                prefix.push_str(&serde_json::to_string(value).map_err(|e| e.to_string())?);
                None
            }
            Source::UnsignedReferenceList {
                bits,
                length,
                items,
            } => {
                prefix.push_str(
                    &serde_json::to_string(&resolved(*bits, *length, items)?)
                        .map_err(|e| e.to_string())?,
                );
                None
            }
            Source::Revision { value } => {
                prefix.push_str(&serde_json::to_string(value).map_err(|e| e.to_string())?);
                None
            }
        };
        writeln!(source, "    line.push_str({prefix:?});").unwrap();
        if let Some(expr) = dynamic {
            writeln!(
                source,
                "    line.push_str(&::serde_json::to_string({expr}).expect(\"serialize string\"));"
            )
            .unwrap();
        }
        obligations.push(serde_json::json!({"declaration":field.id,"index":index,"wire":field.wire,"source":field.source}));
    }
    source.push_str("    line.push('}');\n    println!(\"{line}\");\n    ::std::process::ExitCode::SUCCESS\n}\n");
    let catalog = encode(
        &serde_json::json!({"report":report.id,"fields":obligations,"guards":report.guards}),
    )?;
    let lock = encode(
        &serde_json::json!({"format":report.format,"graphSha256":digest(graph),
        "overlaySha256":digest(overlay),"obligationsSha256":digest(&catalog),"sourceSha256":digest(source.as_bytes()),
        "buildScriptSha256":digest(TARGET_SCRIPT.as_bytes())}),
    )?;
    fs::create_dir(out)
        .and_then(|_| fs::write(out.join("report.rs"), source))
        .and_then(|_| fs::write(out.join("obligations.json"), catalog))
        .and_then(|_| fs::write(out.join("projection.lock"), lock))
        .map_err(|e| e.to_string())
}
