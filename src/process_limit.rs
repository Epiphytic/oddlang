//! Lower a declared process resource policy to the native libc boundary.
use crate::wire_schema::{digest, encode, iri, symbol};
use serde::Deserialize;
use std::{fmt::Write as _, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    resource: Resource,
    platforms: Platforms,
    failure: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Resource {
    OpenDescriptors,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Platforms {
    macos: Policy,
    linux: Policy,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Policy {
    NoOp,
    LowerSoft { ceiling: u32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    entry: String,
    error: ErrorBinding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorBinding {
    id: String,
    #[serde(rename = "type")]
    type_name: String,
    variant: String,
    result: String,
    display: String,
}

pub(crate) fn build(
    graph: &[u8],
    overlay: &[u8],
    out: &Path,
    has_libc: bool,
) -> Result<(), String> {
    let policy: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    if policy.format != "oddlang-process-limit-v1" {
        return Err("invalid process limit format".into());
    }
    iri(&policy.id)?;
    iri(&policy.failure)?;
    let ErrorBinding {
        id,
        type_name,
        variant,
        result,
        display,
    } = binding.error;
    if id != policy.failure || id == policy.id {
        return Err("invalid process limit failure identity".into());
    }
    if ![&binding.entry, &type_name, &variant, &result]
        .into_iter()
        .all(|name| symbol(name))
        || type_name == result
        || display.is_empty()
        || display.chars().any(char::is_control)
    {
        return Err("invalid process limit Rust binding".into());
    }
    let constant = match policy.resource {
        Resource::OpenDescriptors => "::libc::RLIMIT_NOFILE",
    };
    let entry = binding.entry;
    let mut source = format!(
        r#"#[derive(Debug)]
pub enum {type_name} {{
    {variant},
}}
impl ::std::fmt::Display for {type_name} {{
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{
        match self {{
            Self::{variant} => fmt.write_str({display:?}),
        }}
    }}
}}
impl ::std::error::Error for {type_name} {{}}
pub type {result}<T> = ::std::result::Result<T, {type_name}>;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("process limit policy supports Linux and macOS only");
"#
    );
    for (os, platform) in [
        ("linux", policy.platforms.linux),
        ("macos", policy.platforms.macos),
    ] {
        writeln!(
            source,
            "#[cfg(target_os = {os:?})]\npub fn {entry}() -> {result}<()> {{"
        )
        .unwrap();
        match platform {
            Policy::NoOp => source.push_str("    Ok(())\n}\n"),
            Policy::LowerSoft { ceiling } => {
                if ceiling == 0 {
                    return Err("process limit ceiling must be positive".into());
                }
                if !has_libc {
                    return Err("process limit lowering requires native dependency libc".into());
                }
                writeln!(
                    source,
                    r#"    let unavailable = || {type_name}::{variant};
    let read = || {{
        let mut limit = ::libc::rlimit {{ rlim_cur: 0, rlim_max: 0 }};
        if unsafe {{ ::libc::getrlimit({constant}, &mut limit) }} != 0 {{
            return Err(unavailable());
        }}
        Ok(limit)
    }};
    let before = read()?;
    let soft = before.rlim_cur.min(::libc::rlim_t::from({ceiling}u32));
    if before.rlim_cur != soft {{
        let desired = ::libc::rlimit {{ rlim_cur: soft, rlim_max: before.rlim_max }};
        if unsafe {{ ::libc::setrlimit({constant}, &desired) }} != 0 {{
            return Err(unavailable());
        }}
    }}
    let after = read()?;
    if after.rlim_cur != soft || after.rlim_max != before.rlim_max {{
        return Err(unavailable());
    }}
    Ok(())
}}"#
                )
                .unwrap();
            }
        }
    }
    fs::create_dir(out).map_err(|e| e.to_string())?;
    fs::write(out.join("limits.rs"), &source).map_err(|e| e.to_string())?;
    fs::write(
        out.join("projection.lock"),
        encode(&serde_json::json!({
            "format": "oddlang-process-limit-projection-lock-v1",
            "graphSha256": digest(graph),
            "overlaySha256": digest(overlay),
            "sourceSha256": digest(source.as_bytes()),
        }))?,
    )
    .map_err(|e| e.to_string())
}
