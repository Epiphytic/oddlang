//! The rust-facade-v1 producer projects public type paths from private local Rust modules.
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{NamedOrBlankNode, Term},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, String>;
type Exports = BTreeMap<String, BTreeSet<String>>;
const PROFILE: &str = "rust-facade-v1";
const NS: &str = "urn:oddlang:facade:";
const RUSTC: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
const MARMISH_WORKSPACE: &str = "bf726da78321c6f465902f679bbd05557374091458208fe5531be8b3cc2e4f53";
const MARMISH_MANIFEST: &str = "902f311af435a59a6929bc1a55677caa68491ead24aff5f20277884dddc10377";
const MARMISH_LOCK: &str = "9c5f7dde5c0f7d340e22dbefaa8a1c6ddb94c119011ba3e49ac8044c9b4cb0b3";

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn is_sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn json(v: &impl Serialize) -> Result<Vec<u8>> {
    let mut b = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    b.push(b'\n');
    Ok(b)
}
fn read(p: &Path) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(p)
        .map_err(|e| format!("{}: {e}", p.display()))?
        .is_file()
    {
        return Err(format!("expected regular file {}", p.display()));
    }
    fs::read(p).map_err(|e| e.to_string())
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && syn::parse_str::<syn::Ident>(s).is_ok()
        && !s.starts_with("__oddlang")
}
fn validate(exports: &Exports) -> Result<()> {
    let mut names: BTreeSet<&str> = exports.keys().map(String::as_str).collect();
    if exports.is_empty() {
        return Err("empty facade".into());
    }
    for (module, types) in exports {
        if !valid_name(module) || types.is_empty() {
            return Err("invalid or unused facade module".into());
        }
        for n in types {
            if !valid_name(n) || !names.insert(n) {
                return Err("ambiguous or invalid facade export".into());
            }
        }
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    profile: String,
    package: String,
    exports: BTreeMap<String, Vec<String>>,
}
impl Mapping {
    fn expected(&self) -> Result<Exports> {
        if self.profile != PROFILE
            || self.package.is_empty()
            || !self
                .package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("invalid facade mapping".into());
        }
        let mut e = Exports::new();
        for (m, values) in &self.exports {
            let types: BTreeSet<_> = values.iter().cloned().collect();
            if values.len() != types.len() {
                return Err("duplicate facade mapping".into());
            }
            e.insert(m.clone(), types);
        }
        validate(&e)?;
        Ok(e)
    }
}
fn literal_doc(a: &syn::Attribute) -> bool {
    matches!(&a.meta,syn::Meta::NameValue(n) if n.path.is_ident("doc") && matches!(&n.value,syn::Expr::Lit(l) if matches!(&l.lit,syn::Lit::Str(_))))
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Witness {
    byte_start: usize,
    byte_end: usize,
    line: usize,
    column: usize,
    span_sha256: String,
}
fn witness(id: &syn::Ident, source: &[u8]) -> Result<Witness> {
    let span = id.span();
    let r = span.byte_range();
    Ok(Witness {
        line: span.start().line,
        column: span.start().column + 1,
        byte_start: r.start,
        byte_end: r.end,
        span_sha256: sha(source.get(r).ok_or("invalid source coordinate")?),
    })
}
fn module_fact(m: &str) -> String {
    format!("<{NS}root> <{NS}private-module> <{NS}module:{m}> .")
}
fn type_fact(m: &str, n: &str) -> String {
    format!("<{NS}module:{m}> <{NS}public-type> <{NS}type:{m}:{n}> .")
}
fn graph(exports: &Exports) -> String {
    let mut lines = BTreeSet::new();
    for (m, items) in exports {
        lines.insert(module_fact(m));
        for n in items {
            lines.insert(type_fact(m, n));
        }
    }
    lines.into_iter().collect::<Vec<_>>().join("\n") + "\n"
}
fn lower(raw: &[u8]) -> Result<(Exports, BTreeMap<String, Witness>)> {
    let src = syn::parse_file(std::str::from_utf8(raw).map_err(|e| e.to_string())?)
        .map_err(|e| format!("src/lib.rs: {e}"))?;
    if src.shebang.is_some() || src.attrs.iter().any(|a| !literal_doc(a)) {
        return Err("unsupported facade crate attribute".into());
    }
    let mut modules = BTreeSet::new();
    let mut exports = Exports::new();
    let mut sites = BTreeMap::new();
    for item in src.items {
        match item {
            syn::Item::Mod(m)
                if m.attrs.iter().all(literal_doc)
                    && m.vis == syn::Visibility::Inherited
                    && m.unsafety.is_none()
                    && m.content.is_none()
                    && m.semi.is_some() =>
            {
                let name = m.ident.to_string();
                if !modules.insert(name.clone()) {
                    return Err("duplicate facade module".into());
                }
                sites.insert(sha(module_fact(&name).as_bytes()), witness(&m.ident, raw)?);
            }
            syn::Item::Use(u)
                if u.attrs.iter().all(literal_doc)
                    && matches!(u.vis, syn::Visibility::Public(_))
                    && u.leading_colon.is_none() =>
            {
                let syn::UseTree::Path(p) = u.tree else {
                    return Err("expected an explicit local module export".into());
                };
                let m = p.ident.to_string();
                let items = match *p.tree {
                    syn::UseTree::Group(g) => g.items.into_iter().collect(),
                    n => vec![n],
                };
                if items.is_empty() {
                    return Err("empty facade use group".into());
                }
                for i in items {
                    let syn::UseTree::Name(n) = i else {
                        return Err("unsupported facade alias, glob or nested use".into());
                    };
                    let name = n.ident.to_string();
                    if !exports.entry(m.clone()).or_default().insert(name.clone()) {
                        return Err("duplicate facade export".into());
                    }
                    sites.insert(
                        sha(type_fact(&m, &name).as_bytes()),
                        witness(&n.ident, raw)?,
                    );
                }
            }
            _ => return Err("unsupported facade declaration, visibility or cfg".into()),
        }
    }
    if modules.iter().ne(exports.keys()) {
        return Err("facade module declarations and exports differ".into());
    }
    validate(&exports)?;
    Ok((exports, sites))
}
fn decode_graph(bytes: &[u8]) -> Result<Exports> {
    let mut exports = Exports::new();
    let mut modules = BTreeSet::new();
    for q in RdfParser::from_format(RdfFormat::NTriples).for_reader(bytes) {
        let q = q.map_err(|e| e.to_string())?;
        let (NamedOrBlankNode::NamedNode(s), Term::NamedNode(o)) = (q.subject, q.object) else {
            return Err("expected facade IRI".into());
        };
        let s = s
            .as_str()
            .strip_prefix(NS)
            .ok_or("invalid facade subject")?;
        let o = o.as_str().strip_prefix(NS).ok_or("invalid facade object")?;
        match q.predicate.as_str().strip_prefix(NS) {
            Some("private-module") if s == "root" => {
                modules.insert(
                    o.strip_prefix("module:")
                        .ok_or("invalid facade module")?
                        .to_owned(),
                );
            }
            Some("public-type") => {
                let m = s
                    .strip_prefix("module:")
                    .ok_or("invalid facade type scope")?;
                let n = o
                    .strip_prefix(&format!("type:{m}:"))
                    .ok_or("mismatched facade type scope")?;
                exports
                    .entry(m.to_owned())
                    .or_default()
                    .insert(n.to_owned());
            }
            _ => return Err("unexpected facade graph declaration".into()),
        }
    }
    validate(&exports)?;
    if modules.iter().ne(exports.keys()) || graph(&exports).as_bytes() != bytes {
        return Err("noncanonical or incomplete facade graph".into());
    }
    Ok(exports)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Build {
    compiler: String,
    target: String,
    edition: String,
    package: String,
    inputs: BTreeMap<String, String>,
    workspace_sha256: String,
    cargo_lock_sha256: String,
    observed_types: usize,
    witness_sha256: String,
    consumer_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceMap {
    version: u8,
    source: String,
    file_sha256: String,
    file_bytes: usize,
    graph_sha256: String,
    build_sha256: String,
    statement_witnesses: BTreeMap<String, Witness>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lock {
    version: u8,
    profile: String,
    graph_sha256: String,
    files: BTreeMap<String, String>,
    mapping_sha256: String,
    build: Build,
}
fn command(c: &mut Command) -> Result<String> {
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "facade compiler: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}
fn check_sibling(raw: &[u8], names: &BTreeSet<String>, marmish: bool) -> Result<()> {
    let doc = syn::parse_file(std::str::from_utf8(raw).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if doc.shebang.is_some() || doc.attrs.iter().any(|a| !literal_doc(a)) {
        return Err("unsupported facade sibling attributes".into());
    }
    let mut found = BTreeSet::new();
    for item in doc.items {
        if !marmish {
            match &item {
                syn::Item::Struct(s) => {
                    check_data_fields(&s.fields)?;
                    if !s.generics.params.is_empty() || s.generics.where_clause.is_some() {
                        return Err("unsupported standalone generic declaration".into());
                    }
                }
                syn::Item::Enum(s) => {
                    if !s.generics.params.is_empty() || s.generics.where_clause.is_some() {
                        return Err("unsupported standalone generic declaration".into());
                    }
                    for v in &s.variants {
                        if v.discriminant.is_some() || !v.attrs.is_empty() {
                            return Err("unsupported standalone variant".into());
                        }
                        check_data_fields(&v.fields)?;
                    }
                }
                _ => return Err("unsupported standalone facade sibling declaration".into()),
            }
        }
        let (id, vis, attrs, generics) = match &item {
            syn::Item::Struct(s) => (&s.ident, &s.vis, &s.attrs, &s.generics),
            syn::Item::Enum(s) => (&s.ident, &s.vis, &s.attrs, &s.generics),
            _ if marmish => continue,
            _ => return Err("unsupported standalone facade sibling declaration".into()),
        };
        if !marmish && attrs.iter().any(|a| !literal_doc(a)) {
            return Err("unsupported standalone sibling attribute".into());
        }
        if names.contains(&id.to_string()) {
            if !matches!(vis, syn::Visibility::Public(_))
                || !generics.params.is_empty()
                || attrs
                    .iter()
                    .any(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
            {
                return Err("expected unconditional public monomorphic type".into());
            }
            found.insert(id.to_string());
        }
    }
    if &found != names {
        return Err("unresolved public facade type".into());
    }
    Ok(())
}

fn witness_probes(exports: &Exports) -> (String, String) {
    let mut aliases = String::from("\n#[doc(hidden)] pub mod __oddlang_type_identity {\n");
    let mut consumer = String::new();
    let mut index = 0;
    for (module, names) in exports {
        for name in names {
            aliases.push_str(&format!(
                "pub use super::{module}::{name} as Type{index};\n"
            ));
            consumer.push_str(&format!("pub type Original{index} = production::{name};\nfn forward_{index}(x: witnessed::{name}) -> witnessed::__oddlang_type_identity::Type{index} {{ x }}\nfn reverse_{index}(x: witnessed::__oddlang_type_identity::Type{index}) -> witnessed::{name} {{ x }}\n"));
            index += 1;
        }
    }
    aliases.push_str("}\n");
    (aliases, consumer)
}
fn parse_toml(b: &[u8]) -> Result<toml::Table> {
    toml::from_str(std::str::from_utf8(b).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn check_data_fields(fields: &syn::Fields) -> Result<()> {
    for f in fields {
        let syn::Type::Path(p) = &f.ty else {
            return Err("standalone facade fixture expects simple field types".into());
        };
        if f.attrs.iter().any(|a| !literal_doc(a))
            || p.qself.is_some()
            || p.path.get_ident().is_none()
        {
            return Err("standalone facade field or attribute is outside profile".into());
        }
    }
    Ok(())
}
fn native_check(
    manifest: &Path,
    raw: &[u8],
    mapping: &Mapping,
    exports: &Exports,
) -> Result<Build> {
    let root = manifest.parent().ok_or("missing facade package root")?;
    if root.join("src").canonicalize().map_err(|e| e.to_string())? != root.join("src") {
        return Err("redirected facade source directory".into());
    }
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let d = temp.path();
    let tool = |name: &str| {
        command(Command::new("rustup").args(["which", "--toolchain", "1.97.1", name]))
            .map(|s| PathBuf::from(s.trim()))
    };
    let cargo = tool("cargo")?;
    let rustc = tool("rustc")?;
    let compiler = command(Command::new(&rustc).arg("-vV"))?;
    if !compiler
        .lines()
        .any(|line| line == format!("commit-hash: {RUSTC}"))
    {
        return Err("facade requires the pinned 1.97.1 compiler".into());
    }
    let target = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing compiler host")?
        .to_owned();
    let home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".cargo")))
        .ok_or("missing Cargo home")?;
    for file in ["config", "config.toml"] {
        let path = home.join(file);
        if path.exists()
            && parse_toml(&read(&path)?)?.iter().any(|(k, v)| {
                k != "target"
                    || v.as_table().is_none_or(|t| {
                        t.keys().any(|k| {
                            k == &target
                                || !k
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        })
                    })
            })
        {
            return Err("unsupported Cargo host configuration for facade".into());
        }
    }
    let local_config = |p: &Path| {
        [".cargo/config", ".cargo/config.toml"]
            .iter()
            .any(|c| p.join(c).exists())
    };
    if d.ancestors().any(local_config) {
        return Err("unexpected compiler working directory configuration".into());
    }
    let cmd = || {
        let mut c = Command::new(&cargo);
        c.current_dir(d).env_clear();
        for k in [
            "PATH",
            "HOME",
            "CARGO_HOME",
            "TMPDIR",
            "SYSTEMROOT",
            "SDKROOT",
            "DEVELOPER_DIR",
        ] {
            if let Some(v) = env::var_os(k) {
                c.env(k, v);
            }
        }
        c.env("CARGO_NET_OFFLINE", "true")
            .env("RUSTC", &rustc)
            .env("CARGO_TARGET_DIR", d.join("target"));
        c
    };
    let meta: Value = serde_json::from_str(&command(
        cmd()
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--locked",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(manifest),
    )?)
    .map_err(|e| e.to_string())?;
    let ws = Path::new(
        meta["workspace_root"]
            .as_str()
            .ok_or("missing workspace root")?,
    );
    if root
        .ancestors()
        .take_while(|p| p.starts_with(ws))
        .any(local_config)
    {
        return Err("unsupported local Cargo configuration".into());
    }
    let ws_data = read(&ws.join("Cargo.toml"))?;
    let workspace_sha256 = sha(&ws_data);
    let cargo_lock_sha256 = sha(&read(&ws.join("Cargo.lock"))?);
    let manifest_bytes = read(manifest)?;
    let manifest_sha = sha(&manifest_bytes);
    let marmish = manifest_sha == MARMISH_MANIFEST;
    let selected = meta["packages"]
        .as_array()
        .ok_or("missing Cargo packages")?
        .iter()
        .find(|p| p["manifest_path"].as_str() == manifest.to_str())
        .ok_or("facade package missing from workspace")?;
    if selected["name"].as_str() != Some(&mapping.package)
        || selected["edition"] != "2021"
        || selected["features"]
            .as_object()
            .is_none_or(|x| !x.is_empty())
    {
        return Err("facade package, edition or features mismatch".into());
    }
    let targets = selected["targets"]
        .as_array()
        .ok_or("missing Cargo targets")?;
    if targets
        .iter()
        .any(|t| t["kind"] == serde_json::json!(["custom-build"]))
        || !targets.iter().any(|t| {
            t["kind"] == serde_json::json!(["lib"])
                && t["src_path"].as_str() == root.join("src/lib.rs").to_str()
        })
    {
        return Err("unsupported facade target or build script".into());
    }
    if marmish {
        if workspace_sha256 != MARMISH_WORKSPACE || cargo_lock_sha256 != MARMISH_LOCK {
            return Err("changed pinned Marmish build inputs".into());
        }
    } else {
        let manifest = parse_toml(&manifest_bytes)?;
        if manifest
            .keys()
            .any(|k| !["workspace", "package", "dependencies"].contains(&k.as_str()))
            || selected["dependencies"]
                .as_array()
                .is_none_or(|d| !d.is_empty())
            || manifest["package"].as_table().is_none_or(|p| {
                p.keys()
                    .any(|k| !["name", "version", "publish", "edition"].contains(&k.as_str()))
            })
            || parse_toml(&ws_data)?
                .keys()
                .any(|k| ["profile", "patch", "replace"].contains(&k.as_str()))
        {
            return Err(
                "standalone facade fixture requires a dependency-free library and default profile"
                    .into(),
            );
        }
    }
    let mut inputs = BTreeMap::from([
        ("Cargo.toml".to_owned(), manifest_sha),
        ("src/lib.rs".to_owned(), sha(raw)),
    ]);
    let witness_dir = d.join("witness");
    fs::create_dir(&witness_dir).map_err(|e| e.to_string())?;
    for (m, names) in exports {
        let path = format!("src/{m}.rs");
        let blob = read(&root.join(&path))?;
        let hash = sha(&blob);
        if marmish
            && !matches!(
                (m.as_str(), hash.as_str()),
                (
                    "config",
                    "2d39557b91c89de5c7790a53e35fd76f3ab94fbb5a8df43ed348da749cc81140"
                ) | (
                    "frame",
                    "3023d3947b16d6d108cf741a057df7214dba9a2985d6b0e3e9a6703af02576ba"
                )
            )
        {
            return Err("unrecognized Marmish sibling source for facade".into());
        }
        check_sibling(&blob, names, marmish)?;
        fs::write(witness_dir.join(format!("{m}.rs")), blob).map_err(|e| e.to_string())?;
        inputs.insert(path, hash);
    }
    let build = command(
        cmd()
            .args([
                "check",
                "--lib",
                "--locked",
                "--offline",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(manifest)
            .arg("-p")
            .arg(&mapping.package),
    )?;
    let mut libs = BTreeMap::new();
    for line in build.lines() {
        let event: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if event["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(paths) = event["filenames"].as_array() else {
            continue;
        };
        if let Some(p) = paths
            .iter()
            .filter_map(Value::as_str)
            .find(|p| p.ends_with(".rmeta"))
        {
            libs.insert(
                event["target"]["name"]
                    .as_str()
                    .ok_or("unidentified library")?
                    .to_owned(),
                PathBuf::from(p),
            );
        }
    }
    let selected_lib = targets
        .iter()
        .find(|t| t["kind"] == serde_json::json!(["lib"]))
        .and_then(|t| t["name"].as_str())
        .ok_or("missing library name")?;
    let prod = libs
        .get(selected_lib)
        .ok_or("Cargo produced no facade library metadata")?;
    let search = format!(
        "dependency={}",
        prod.parent()
            .ok_or("missing dependency directory")?
            .display()
    );
    let (aliases, consumer) = witness_probes(exports);
    let mut augmented = raw.to_vec();
    augmented.extend(aliases.as_bytes());
    fs::write(witness_dir.join("lib.rs"), augmented).map_err(|e| e.to_string())?;
    let witness_meta = d.join("libwitnessed.rmeta");
    let compiler_cmd = || {
        let mut c = Command::new(&rustc);
        c.current_dir(d).args([
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            "-Awarnings",
            "-L",
            &search,
        ]);
        c
    };
    let mut instrument = compiler_cmd();
    for dep in selected["dependencies"]
        .as_array()
        .ok_or("invalid dependency declarations")?
    {
        let n = dep["name"]
            .as_str()
            .ok_or("missing dependency")?
            .replace('-', "_");
        let path = libs
            .get(&n)
            .ok_or_else(|| format!("missing resolved dependency {n}"))?;
        instrument
            .arg("--extern")
            .arg(format!("{n}={}", path.display()));
    }
    command(
        instrument
            .args(["--crate-name", "witnessed"])
            .arg(witness_dir.join("lib.rs"))
            .arg("-o")
            .arg(&witness_meta),
    )?;
    let probe = d.join("consumer.rs");
    fs::write(&probe, &consumer).map_err(|e| e.to_string())?;
    command(
        compiler_cmd()
            .args(["--crate-name", "facade_consumer", "--extern"])
            .arg(format!("production={}", prod.display()))
            .arg("--extern")
            .arg(format!("witnessed={}", witness_meta.display()))
            .arg(probe)
            .arg("-o")
            .arg(d.join("consumer.rmeta")),
    )?;
    for (path, h) in &inputs {
        if &sha(&read(&root.join(path))?) != h {
            return Err("facade source changed during compilation".into());
        }
    }
    if workspace_sha256 != sha(&read(&ws.join("Cargo.toml"))?)
        || cargo_lock_sha256 != sha(&read(&ws.join("Cargo.lock"))?)
    {
        return Err("facade workspace changed during compilation".into());
    }
    Ok(Build {
        compiler,
        target,
        edition: "2021".into(),
        package: mapping.package.clone(),
        inputs,
        workspace_sha256,
        cargo_lock_sha256,
        observed_types: exports.values().map(|v| v.len()).sum(),
        witness_sha256: sha(aliases.as_bytes()),
        consumer_sha256: sha(consumer.as_bytes()),
    })
}

fn publish(out: &Path, files: Vec<(&str, Vec<u8>)>) -> Result<()> {
    if out.exists() {
        return Err("facade publication requires an absent output".into());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, bytes) in files {
        fs::write(tmp.path().join(name), bytes).map_err(|e| e.to_string())?;
    }
    fs::rename(tmp.path(), out).map_err(|e| e.to_string())
}
pub fn import(manifest: &Path, mapping: &Path, out: &Path) -> Result<()> {
    let map = read(mapping)?;
    let settings: Mapping = toml::from_str(std::str::from_utf8(&map).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let expected = settings.expected()?;
    if !fs::symlink_metadata(manifest)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("expected regular Cargo manifest".into());
    }
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    let root = manifest.parent().ok_or("missing facade crate")?;
    let source = read(&root.join("src/lib.rs"))?;
    let (exports, witnesses) = lower(&source)?;
    if expected != exports {
        return Err("facade identities differ from pinned mapping".into());
    }
    let build = native_check(&manifest, &source, &settings, &exports)?;
    let graph = graph(&exports).into_bytes();
    let graph_sha256 = sha(&graph);
    let map_bytes = json(&SourceMap {
        version: 1,
        source: "src/lib.rs".into(),
        file_sha256: sha(&source),
        file_bytes: source.len(),
        graph_sha256: graph_sha256.clone(),
        build_sha256: sha(&json(&build)?),
        statement_witnesses: witnesses,
    })?;
    let files = BTreeMap::from([
        ("domain.ttl".into(), graph_sha256.clone()),
        ("source-map.json".into(), sha(&map_bytes)),
    ]);
    let lock = Lock {
        version: 1,
        profile: PROFILE.into(),
        graph_sha256,
        files,
        mapping_sha256: sha(&map),
        build,
    };
    publish(
        out,
        vec![
            ("domain.ttl", graph),
            ("source-map.json", map_bytes),
            ("import.lock", json(&lock)?),
        ],
    )
}
fn load_pinned(dir: &Path, pin: &str) -> Result<(Exports, String)> {
    let mut names = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|f| f.file_name()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    names.sort();
    if names != ["domain.ttl", "import.lock", "source-map.json"] {
        return Err("unexpected facade import inventory".into());
    }
    let lock_bytes = read(&dir.join("import.lock"))?;
    if !is_sha(pin) || sha(&lock_bytes) != pin {
        return Err("external facade import pin mismatch".into());
    }
    let lock: Lock = serde_json::from_slice(&lock_bytes).map_err(|e| e.to_string())?;
    let facts = read(&dir.join("domain.ttl"))?;
    let graph_hash = sha(&facts);
    let m = read(&dir.join("source-map.json"))?;
    let map: SourceMap = serde_json::from_slice(&m).map_err(|e| e.to_string())?;
    let files = BTreeMap::from([
        ("domain.ttl".to_owned(), graph_hash.clone()),
        ("source-map.json".to_owned(), sha(&m)),
    ]);
    let b = &lock.build;
    if lock.version != 1
        || lock.profile != PROFILE
        || lock.graph_sha256 != graph_hash
        || json(&lock)? != lock_bytes
        || !is_sha(&lock.mapping_sha256)
        || files != lock.files
        || !b
            .compiler
            .lines()
            .any(|l| l == format!("commit-hash: {RUSTC}"))
        || !b
            .compiler
            .lines()
            .any(|l| l == format!("host: {}", b.target))
        || b.edition != "2021"
        || !is_sha(&b.workspace_sha256)
        || !is_sha(&b.cargo_lock_sha256)
        || b.inputs.values().any(|h| !is_sha(h))
    {
        return Err("invalid facade import lock".into());
    }
    let exports = decode_graph(&facts)?;
    let mut expected_inputs: BTreeSet<_> = exports.keys().map(|m| format!("src/{m}.rs")).collect();
    expected_inputs.extend(["src/lib.rs".into(), "Cargo.toml".into()]);
    let mut expected_facts = BTreeMap::new();
    for (module, types) in &exports {
        expected_facts.insert(sha(module_fact(module).as_bytes()), module);
        for name in types {
            expected_facts.insert(sha(type_fact(module, name).as_bytes()), name);
        }
    }
    let (aliases, consumer) = witness_probes(&exports);
    if expected_inputs.iter().ne(b.inputs.keys())
        || map.version != 1
        || map.source != "src/lib.rs"
        || Some(&map.file_sha256) != b.inputs.get(&map.source)
        || map.build_sha256 != sha(&json(b)?)
        || map.graph_sha256 != graph_hash
        || json(&map)? != m
        || expected_facts.keys().ne(map.statement_witnesses.keys())
        || map.statement_witnesses.iter().any(|(fact, s)| {
            let name = expected_facts[fact];
            s.span_sha256 != sha(name.as_bytes())
                || s.byte_end.checked_sub(s.byte_start) != Some(name.len())
                || s.byte_end <= s.byte_start
                || s.byte_end > map.file_bytes
                || s.line == 0
                || s.column == 0
        })
        || b.observed_types != exports.values().map(|s| s.len()).sum::<usize>()
        || b.witness_sha256 != sha(aliases.as_bytes())
        || b.consumer_sha256 != sha(consumer.as_bytes())
    {
        return Err("facade provenance or identity witnesses disagree".into());
    }
    Ok((exports, graph_hash))
}
pub fn verify_pinned(dir: &Path, pin: &str) -> Result<()> {
    load_pinned(dir, pin).map(|_| ())
}
pub fn build_pinned(dir: &Path, pin: &str, out: &Path) -> Result<()> {
    use std::fmt::Write;
    let (exports, graph_hash) = load_pinned(dir, pin)?;
    let mut rust = String::from("// Generated from the locked public type facade.\n");
    for m in exports.keys() {
        writeln!(rust, "mod {m};").unwrap();
    }
    rust.push('\n');
    for (m, names) in exports {
        for n in names {
            writeln!(rust, "pub use {m}::{n};").unwrap();
        }
    }
    let manifest = serde_json::json!({"version":1,"graphSha256":graph_hash,"files":{"lib.rs":sha(rust.as_bytes())}});
    publish(
        out,
        vec![
            ("lib.rs", rust.into_bytes()),
            ("projection.json", json(&manifest)?),
        ],
    )
}
