//! Lower a declared process resource policy to the native libc boundary.
use crate::wire_schema::{digest, encode, iri, symbol};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

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
    NoOp {},
    LowerSoft { ceiling: u32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    entry: String,
    error: ErrorBinding,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorBinding {
    Local(LocalError),
    Declared(DeclaredError),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeclaredError {
    id: String,
    type_path: Vec<String>,
    variant: String,
}

impl DeclaredError {
    fn type_path(&self) -> Option<String> {
        (!self.type_path.is_empty()
            && self.type_path.iter().all(|part| symbol(part))
            && symbol(&self.variant))
        .then(|| format!("crate::{}", self.type_path.join("::")))
    }

    pub(crate) fn validate_link(
        &self,
        providers: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<(), String> {
        let ty = self
            .type_path()
            .ok_or("invalid declared process limit type path")?;
        let variant = format!("{ty}::{}", self.variant);
        if !providers
            .get(&self.id)
            .is_some_and(|paths| paths.contains(&variant))
        {
            return Err(format!(
                "unresolved declared process limit error: {}",
                self.id
            ));
        }
        Ok(())
    }
}

pub(crate) fn declared_error(overlay: &[u8]) -> Result<Option<DeclaredError>, String> {
    let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    Ok(match binding.error {
        ErrorBinding::Declared(error) => Some(error),
        ErrorBinding::Local(_) => None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalError {
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
    let constant = match policy.resource {
        Resource::OpenDescriptors => "::libc::RLIMIT_NOFILE",
    };
    let entry = binding.entry;
    let id = match &binding.error {
        ErrorBinding::Local(error) => &error.id,
        ErrorBinding::Declared(error) => &error.id,
    };
    if *id != policy.failure || id == &policy.id {
        return Err("invalid process limit failure identity".into());
    }
    if !symbol(&entry) {
        return Err("invalid process limit Rust binding".into());
    }
    let (mut source, result, failure) = match binding.error {
        ErrorBinding::Local(LocalError {
            type_name,
            variant,
            result,
            display,
            ..
        }) => {
            if ![&type_name, &variant, &result]
                .into_iter()
                .all(|name| symbol(name))
                || type_name == result
                || display.is_empty()
                || display.chars().any(char::is_control)
            {
                return Err("invalid process limit Rust binding".into());
            }
            let source = format!(
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

"#
            );
            (
                source,
                format!("{result}<()>"),
                format!("{type_name}::{variant}"),
            )
        }
        ErrorBinding::Declared(error) => {
            let ty = error
                .type_path()
                .ok_or("invalid declared process limit type path")?;
            (
                String::new(),
                format!("::core::result::Result<(), {ty}>"),
                format!("{ty}::{}", error.variant),
            )
        }
    };
    source.push_str(
        r#"#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("process limit policy supports Linux and macOS only");
"#,
    );
    for (os, platform) in [
        ("linux", policy.platforms.linux),
        ("macos", policy.platforms.macos),
    ] {
        writeln!(
            source,
            "#[cfg(target_os = {os:?})]\npub fn {entry}() -> {result} {{"
        )
        .unwrap();
        match platform {
            Policy::NoOp {} => source.push_str("    Ok(())\n}\n"),
            Policy::LowerSoft { ceiling } => {
                if ceiling == 0 {
                    return Err("process limit ceiling must be positive".into());
                }
                if !has_libc {
                    return Err("process limit lowering requires native dependency libc".into());
                }
                writeln!(
                    source,
                    r#"    let unavailable = || {failure};
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
