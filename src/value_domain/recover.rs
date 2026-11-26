//! Derive a candidate interval and named errors through the selected native public API.
use super::{Binding, Result, digest};
use oxigraph::model::{Literal, NamedNode};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    source_revision: String,
    cargo_lock_sha256: String,
    compiler_commit: String,
    package: String,
    #[serde(default)]
    bits: u8,
    #[serde(default)]
    range_error: String,
    api: Binding,
    source_module: Option<String>,
}

fn output(cmd: &mut Command) -> Result<Vec<u8>> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .arg("--no-replace-objects")
        .current_dir(root);
    command
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(output(git(root).args(args))?)
        .map(|text| text.trim_end_matches('\n').to_owned())
        .map_err(|e| e.to_string())
}

fn clean(root: &Path, revision: &str) -> Result<()> {
    if git_text(root, &["rev-parse", "HEAD"])? != revision
        || !output(git(root).args(["status", "--porcelain", "--untracked-files=all"]))?.is_empty()
    {
        return Err("original checkout does not match the clean revision".into());
    }
    Ok(())
}

fn check_export_links(root: &Path, directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_symlink() {
            if !path
                .canonicalize()
                .is_ok_and(|target| target.starts_with(root))
            {
                return Err(format!(
                    "export link escapes the pinned source: {}",
                    path.display()
                ));
            }
        } else if kind.is_dir() {
            check_export_links(root, &path)?;
        }
    }
    Ok(())
}

fn export(original: &Path, stage: &Path, revision: &str) -> Result<PathBuf> {
    // The alternate object store supplies Git objects only. Archive attributes and
    // other local configuration from the original checkout do not follow them.
    let objects = git_text(
        original,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects",
        ],
    )?;
    let bare = stage.join("objects.git");
    output(
        git(stage)
            .args(["init", "--bare", "--template="])
            .arg(&bare),
    )?;
    fs::create_dir_all(bare.join("objects/info")).map_err(|e| e.to_string())?;
    fs::write(bare.join("objects/info/alternates"), format!("{objects}\n"))
        .map_err(|e| e.to_string())?;
    let archive = stage.join("source.tar");
    output(
        git(&bare)
            .args(["archive", "--format=tar", "--output"])
            .arg(&archive)
            .arg(revision),
    )?;
    let exported = stage.join("source");
    fs::create_dir(&exported).map_err(|e| e.to_string())?;
    output(
        Command::new("tar")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-xf"])
            .arg(archive)
            .current_dir(&exported),
    )?;
    check_export_links(&exported, &exported)?;
    Ok(exported)
}

fn reject_cargo_configs(root: &Path) -> Result<()> {
    for parent in root.ancestors() {
        for name in ["config", "config.toml"] {
            let path = parent.join(".cargo").join(name);
            match path.symlink_metadata() {
                Ok(_) => {
                    return Err(format!(
                        "unexpected Cargo configuration: {}",
                        path.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
use std::os::unix::fs::symlink as symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;

fn isolated_cache(stage: &Path) -> Result<PathBuf> {
    let cache = stage.join("cargo");
    fs::create_dir(&cache).map_err(|e| e.to_string())?;
    let installed = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(installed) = installed {
        // Re-extract crate archives and Git checkouts into the isolated home.
        // Their persistent download caches remain available without source overlays.
        fs::create_dir(cache.join("registry")).map_err(|e| e.to_string())?;
        fs::create_dir(cache.join("git")).map_err(|e| e.to_string())?;
        for name in ["registry/cache", "registry/index", "git/db"] {
            let path = installed.join(name);
            match path.canonicalize() {
                Ok(source) if source.is_dir() => {
                    symlink_dir(source, cache.join(name)).map_err(|e| e.to_string())?;
                }
                Ok(_) => {
                    return Err(format!(
                        "Cargo cache is not a directory: {}",
                        path.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(cache)
}

fn installed_program(name: &str) -> Result<PathBuf> {
    let bytes = output(Command::new("rustup").args(["which", "--toolchain", "1.97.1", name]))?;
    let path = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    PathBuf::from(path.trim_end_matches('\n'))
        .canonicalize()
        .map_err(|e| e.to_string())
}

fn tool(executable: &Path, root: &Path, cache: &Path, rustc: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CARGO_HOME", cache)
        .env("RUSTC", rustc)
        .current_dir(root);
    command
}

fn check_workspace_paths(packages: &[Value], exported: &Path) -> Result<()> {
    for package in packages {
        let paths = package["targets"]
            .as_array()
            .ok_or("missing Cargo targets")?
            .iter()
            .map(|target| &target["src_path"])
            .chain(std::iter::once(&package["manifest_path"]))
            .chain(
                package["dependencies"]
                    .as_array()
                    .ok_or("missing Cargo dependencies")?
                    .iter()
                    .map(|dependency| &dependency["path"])
                    .filter(|path| !path.is_null()),
            );
        for value in paths {
            let path = Path::new(value.as_str().ok_or("invalid Cargo source path")?);
            if !path
                .canonicalize()
                .is_ok_and(|actual| actual.starts_with(exported))
            {
                return Err(format!(
                    "Cargo path escapes the pinned source: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn probe(selection: &Selection) -> Result<String> {
    let api = &selection.api;
    api.validate()?;
    let range = api
        .variants
        .get(&selection.range_error)
        .ok_or("unknown range error")?;
    let mut template = include_str!("probe.rs.txt").to_owned();
    for (key, value) in [
        ("@TYPE@", &api.type_name),
        ("@ERROR@", &api.error_type),
        ("@CONSTRUCTOR@", &api.constructor),
        ("@ACCESSOR@", &api.accessor),
        ("@MAX@", &api.maximum_constant),
        ("@RANGE@", range),
    ] {
        template = template.replace(key, value);
    }
    let ordinal_checks = api.variant_order.as_ref().map_or_else(String::new, |order| {
        order.iter().enumerate().map(|(ordinal, id)|
            format!("    assert_eq!(Error::{} as isize, {ordinal}, \"unexpected enum ordinal\");\n", api.variants[id])
        ).collect()
    });
    Ok(template
        .replace("@ORDINAL_CHECKS@", &ordinal_checks)
        .replace("@INTEGER@", &format!("u{}", selection.bits))
        .replace(
            "@VARIANTS@",
            &api.variants
                .values()
                .map(|v| format!("Error::{v}"))
                .collect::<Vec<_>>()
                .join(","),
        ))
}

pub fn recover_structure(manifest: &Path, selection: &Path, destination: &Path) -> Result<()> {
    recover_mode(manifest, selection, destination, true)
}

fn source_module(
    package: &Value,
    exported: &Path,
    selection: &Selection,
) -> Result<(String, String)> {
    let module = selection
        .source_module
        .as_deref()
        .ok_or("structural recovery requires sourceModule")?;
    if syn::parse_str::<syn::Ident>(module).is_err() || module.starts_with("r#") {
        return Err("invalid source module name".into());
    }
    let targets = package["targets"]
        .as_array()
        .ok_or("missing package targets")?;
    let libraries = targets
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib" || kind == "rlib"))
        })
        .collect::<Vec<_>>();
    let [library] = libraries.as_slice() else {
        return Err("expected one package library".into());
    };
    let root = Path::new(
        library["src_path"]
            .as_str()
            .ok_or("missing package library source")?,
    );
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !root.starts_with(exported) {
        return Err("library source escapes export".into());
    }
    let source = fs::read_to_string(&root).map_err(|e| e.to_string())?;
    let parsed = syn::parse_file(&source).map_err(|e| e.to_string())?;
    super::structural::check_scope(&parsed)?;
    let declarations = parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(item) if item.ident == module => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [item] = declarations.as_slice() else {
        return Err("expected one direct source module".into());
    };
    if item.content.is_some()
        || item.attrs.iter().any(|a| {
            a.path().is_ident("cfg") || a.path().is_ident("cfg_attr") || a.path().is_ident("path")
        })
    {
        return Err("expected a conventional unconditional external source module".into());
    }
    fn direct_exports(tree: &syn::UseTree, names: &mut Vec<String>) {
        match tree {
            syn::UseTree::Name(name) => names.push(name.ident.to_string()),
            syn::UseTree::Group(group) => group
                .items
                .iter()
                .for_each(|item| direct_exports(item, names)),
            _ => (),
        }
    }
    let mut public = Vec::new();
    for item in parsed.items {
        if let syn::Item::Use(item) = item
            && matches!(item.vis, syn::Visibility::Public(_))
            && item
                .attrs
                .iter()
                .all(|a| !a.path().is_ident("cfg") && !a.path().is_ident("cfg_attr"))
            && item.leading_colon.is_none()
            && let syn::UseTree::Path(path) = item.tree
            && path.ident == module
        {
            direct_exports(&path.tree, &mut public);
        }
    }
    if !public.contains(&selection.api.type_name) || !public.contains(&selection.api.error_type) {
        return Err("selected value and error must be direct public module re-exports".into());
    }
    let file = root
        .parent()
        .ok_or("library source has no parent")?
        .join(format!("{module}.rs"));
    let file = file.canonicalize().map_err(|e| e.to_string())?;
    let relative = file
        .strip_prefix(exported)
        .map_err(|_| "source module escapes export")?;
    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    Ok((relative.to_string_lossy().into_owned(), text))
}

fn render_graph(
    s: &Selection,
    bounds: [u64; 3],
    messages: &std::collections::BTreeMap<String, String>,
) -> String {
    let [min, max, default] = bounds;
    let mut graph = format!(
        "@prefix c: <urn:oddlang:core:> .\n<{}> a c:OptionalUnsignedRange ; c:bits {} ; c:minimum {min} ; c:maximum {max} ; c:defaultValue {default} ; c:rangeError <{}> .\n",
        s.api.domain, s.bits, s.range_error
    );
    for (id, message) in messages {
        let literal = Literal::new_simple_literal(message);
        graph.push_str(&format!("<{id}> a c:NamedError ; c:message {literal} .\n"));
    }
    graph
}

pub fn recover(manifest: &Path, selection_path: &Path, destination: &Path) -> Result<()> {
    recover_mode(manifest, selection_path, destination, false)
}

fn recover_mode(
    manifest: &Path,
    selection_path: &Path,
    destination: &Path,
    structural: bool,
) -> Result<()> {
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    if manifest.file_name().and_then(|s| s.to_str()) != Some("Cargo.toml") {
        return Err("expected Cargo.toml".into());
    }
    let input = fs::read(selection_path).map_err(|e| e.to_string())?;
    let mut s: Selection = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    if structural != s.source_module.is_some() {
        return Err("sourceModule is for structural value-domain recovery".into());
    }
    if structural {
        if s.bits != 0 || !s.range_error.is_empty() {
            return Err("structural recovery derives bits and rangeError from the source".into());
        }
    } else if !matches!(s.bits, 8 | 16 | 32 | 64) {
        return Err("invalid unsigned width".into());
    }
    for id in std::iter::once(&s.api.domain).chain(s.api.variants.keys()) {
        NamedNode::new(id.as_str()).map_err(|e| e.to_string())?;
    }
    if s.package.is_empty()
        || !s
            .package
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err("invalid package name".into());
    }
    let manifest_parent = manifest.parent().ok_or("missing manifest parent")?;
    let checkout = PathBuf::from(git_text(
        manifest_parent,
        &["rev-parse", "--show-toplevel"],
    )?)
    .canonicalize()
    .map_err(|e| e.to_string())?;
    clean(&checkout, &s.source_revision)?;
    let source_tree = git_text(&checkout, &["rev-parse", "HEAD^{tree}"])?;
    let relative_manifest = manifest
        .strip_prefix(&checkout)
        .map_err(|e| e.to_string())?;
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let work = stage.path().canonicalize().map_err(|e| e.to_string())?;
    let exported = export(&checkout, &work, &s.source_revision)?;
    let manifest = exported.join(relative_manifest);
    let root = manifest
        .parent()
        .ok_or("missing exported manifest parent")?;
    reject_cargo_configs(root)?;
    let cache = isolated_cache(&work)?;
    let cargo = installed_program("cargo")?;
    let rustc = installed_program("rustc")?;
    let metadata: Value = serde_json::from_slice(&output(
        tool(&cargo, root, &cache, &rustc)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?)
    .map_err(|e| e.to_string())?;
    let workspace = Path::new(
        metadata["workspace_root"]
            .as_str()
            .ok_or("missing workspace root")?,
    );
    if !workspace
        .canonicalize()
        .is_ok_and(|path| path.starts_with(&exported))
    {
        return Err("Cargo workspace escapes the pinned source".into());
    }
    let lock = workspace.join("Cargo.lock");
    if digest(&fs::read(&lock).map_err(|e| e.to_string())?) != s.cargo_lock_sha256 {
        return Err("Cargo.lock pin mismatch".into());
    }
    let packages = metadata["packages"]
        .as_array()
        .ok_or("invalid Cargo metadata")?;
    check_workspace_paths(packages, &exported)?;
    let package = packages
        .iter()
        .find(|p| p["name"] == s.package)
        .ok_or("unknown workspace package")?;
    let from_source = if structural {
        let (module_path, text) = source_module(package, &exported, &s)?;
        let facts = super::structural::derive(&text, &s.api)?;
        s.bits = facts.bits;
        s.range_error = facts.range_error;
        Some((
            render_graph(&s, facts.bounds, &facts.errors),
            module_path,
            digest(text.as_bytes()),
        ))
    } else {
        None
    };
    let program = probe(&s)?;
    let probe_sha256 = digest(program.as_bytes());
    let compiler = String::from_utf8(output(tool(&rustc, root, &cache, &rustc).arg("-vV"))?)
        .map_err(|e| e.to_string())?;
    if !compiler
        .lines()
        .any(|l| l.strip_prefix("commit-hash: ") == Some(&s.compiler_commit))
    {
        return Err("compiler pin mismatch".into());
    }
    let host = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing compiler host")?;
    let target = work.join("target");
    let messages = output(
        tool(&cargo, root, &cache, &rustc)
            .args([
                "build",
                "--release",
                "--locked",
                "--offline",
                "--lib",
                "--message-format=json",
                "--target",
                host,
                "--package",
                &s.package,
                "--manifest-path",
            ])
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target),
    )?;
    let mut libs = Vec::new();
    for row in messages.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
        let event: Value = serde_json::from_slice(row).map_err(|e| e.to_string())?;
        if event["reason"] == "compiler-artifact"
            && event["package_id"] == package["id"]
            && event["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|x| x == "lib" || x == "rlib"))
        {
            libs.extend(
                event["filenames"]
                    .as_array()
                    .ok_or("Cargo artifact filenames missing")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|p| p.ends_with(".rlib"))
                    .map(PathBuf::from),
            );
        }
    }
    let [lib] = libs.as_slice() else {
        return Err("expected one original rlib".into());
    };
    let source = stage.path().join("observe.rs");
    let exe = stage.path().join("observe");
    fs::write(&source, program).map_err(|e| e.to_string())?;
    output(
        tool(&rustc, root, &cache, &rustc)
            .arg("--edition=2024")
            .arg(&source)
            .arg("--extern")
            .arg(format!("observed={}", lib.display()))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                target.join(host).join("release/deps").display()
            ))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                target.join("release/deps").display()
            ))
            .arg("-o")
            .arg(&exe),
    )?;
    let data = String::from_utf8(output(
        Command::new(&exe).env_clear().current_dir(stage.path()),
    )?)
    .map_err(|e| e.to_string())?;
    let mut lines = data.lines();
    let bounds = lines
        .next()
        .ok_or("missing observed interval")?
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let [min, max, default] = bounds.as_slice() else {
        return Err("invalid observed interval".into());
    };
    let mut messages = std::collections::BTreeMap::new();
    for id in s.api.variants.keys() {
        let raw = lines.next().ok_or("missing error display")?;
        if !raw.bytes().all(|b| b.is_ascii_hexdigit()) || raw.len() % 2 != 0 {
            return Err("invalid error display".into());
        }
        let msg = (0..raw.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&raw[i..i + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        messages.insert(
            id.clone(),
            String::from_utf8(msg).map_err(|e| e.to_string())?,
        );
    }
    if lines.next().is_some() {
        return Err("unexpected observations".into());
    }
    let observed = render_graph(&s, [*min, *max, *default], &messages);
    let (graph, structure) = match from_source {
        Some((graph, module, sha256)) => {
            if graph != observed {
                return Err("structural range and sealed native API observation disagree".into());
            }
            (
                graph,
                Some(
                    serde_json::json!({"method":"rust-optional-unsigned-interval-v1","module":module,
                                        "sourceSha256":sha256,"independentNativeAgreement":true}),
                ),
            )
        }
        None => (observed, None),
    };
    clean(&checkout, &s.source_revision)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(tmp.path().join("domain.ttl"), graph).map_err(|e| e.to_string())?;
    let domain = super::load(tmp.path())?;
    let mut receipt = serde_json::json!({"qualification":"candidate","sourceRevision":s.source_revision,"sourceTree":source_tree,"probeSha256":probe_sha256,"compilerCommit":s.compiler_commit,"cargoLockSha256":s.cargo_lock_sha256,"host":host,"profile":"release","package":s.package,"selectionSha256":digest(&input),"graphSha256":digest(&serde_json::to_vec(&domain).map_err(|e|e.to_string())?),"numericCoverage":if s.bits<=16 {"exhaustive"} else {"sampled; interval property requires independent review"}});
    if let Some(structure) = structure {
        receipt["structuralDerivation"] = structure;
    }
    fs::write(
        tmp.path().join("observation.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if destination.symlink_metadata().is_ok() {
        return Err("destination exists".into());
    }
    fs::rename(tmp.path(), destination).map_err(|e| e.to_string())
}
