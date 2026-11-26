//! Project an ordered JSON report of immutable release facts.
use crate::local_command_client::{Callback, Guard};
use crate::wire_schema::{digest, encode, iri, symbol};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

pub(crate) const TARGET_SCRIPT: &str = r#"fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").expect("Cargo target");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output"));
    std::fs::write(out.join("oddlang-build-target.rs"), format!("{target:?}")).expect("write build target");
}
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format: String,
    id: String,
    fields: Vec<Field>,
    #[serde(default)]
    guards: Vec<Guard>,
}

#[derive(Deserialize)]
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
    PackageVersion,
    BuildTarget,
    Revision {
        value: String,
    },
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
