//! Reconcile release compiler and same-toolchain Rustdoc declarations.
mod comparison;
mod document;
use document::Document;
mod profile;
mod spans;

use clap::Parser;
use provider_validation::{Result, read, sha};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const TOOLCHAIN: &str = "1.97.1";
const COMMIT: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";

#[derive(Parser)]
struct Args {
    workspace: PathBuf,
    package: String,
    #[arg(value_parser=["lib","bin"])]
    kind: String,
    target: String,
    triple: String,
    #[arg(long)]
    no_default_features: bool,
    #[arg(long,num_args=1..)]
    features: Vec<String>,
    #[arg(long)]
    public_inherent: bool,
    #[arg(long)]
    revision: Option<String>,
    #[arg(long)]
    previous_spans: Option<PathBuf>,
    #[arg(long)]
    out: PathBuf,
}
fn arr(v: &Value) -> Result<&Vec<Value>> {
    Ok(v.as_array().ok_or("expected an array")?)
}
fn txt(v: &Value) -> Result<&str> {
    Ok(v.as_str().ok_or("expected text")?)
}
fn strpath(p: &Path) -> Result<&str> {
    Ok(p.to_str().ok_or("non UTF-8 path")?)
}
fn strings(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| (*s).into()).collect()
}
fn exec(cmd: &mut Command) -> Result<(Vec<u8>, String)> {
    let o = cmd.output()?;
    let log = String::from_utf8(o.stderr)?;
    if !o.status.success() {
        let tail = log.chars().count().saturating_sub(3000);
        return Err(format!(
            "{:?} failed: {}",
            cmd.get_program(),
            log.chars().skip(tail).collect::<String>()
        )
        .into());
    }
    Ok((o.stdout, log))
}
fn stdout(cmd: &mut Command) -> Result<String> {
    Ok(String::from_utf8(exec(cmd)?.0)?.trim().into())
}
fn tool(name: &str) -> Result<String> {
    stdout(Command::new("rustup").args(["which", "--toolchain", TOOLCHAIN, name]))
}
fn tool_identity(rustc: &str, rustdoc: &str) -> Result<Value> {
    let mut tools = json!({"commit":COMMIT,"release":TOOLCHAIN});
    for (name, path) in [("rustc", rustc), ("rustdoc", rustdoc)] {
        let version = stdout(Command::new(path).arg("-vV"))?;
        let fields: BTreeMap<_, _> = version.lines().filter_map(|l| l.split_once(": ")).collect();
        if fields.get("commit-hash") != Some(&COMMIT) || fields.get("release") != Some(&TOOLCHAIN) {
            return Err(format!("unexpected {name} release").into());
        }
        let host = *fields.get("host").ok_or("missing toolchain host")?;
        if !tools["host"].is_null() && tools["host"] != host {
            return Err("toolchain hosts differ".into());
        }
        tools["host"] = json!(host);
        tools[format!("{name}Sha256")] = json!(sha(Path::new(path))?);
    }
    Ok(tools)
}
type Env = BTreeMap<String, String>;
fn isolated(root: &Path, env: &Env, cmd: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(cmd);
    c.env_clear().envs(env).current_dir(root);
    c
}
fn pinned(root: &Path, revision: Option<&str>) -> Result<Value> {
    let Some(revision) = revision else {
        return Ok(Value::Null);
    };
    let refs =
        stdout(
            Command::new("git")
                .current_dir(root)
                .args(["rev-parse", "HEAD", "HEAD^{tree}"]),
        )?;
    let mut lines = refs.lines();
    if lines.next() != Some(revision) {
        return Err("source revision differs from pin".into());
    }
    if !stdout(Command::new("git").current_dir(root).args([
        "status",
        "--porcelain",
        "--untracked-files=all",
    ]))?
    .is_empty()
    {
        return Err("source checkout must be clean".into());
    }
    Ok(json!({"revision":revision,"tree":lines.next().ok_or("missing pinned tree")?}))
}

fn relative(path: &Path, root: &Path) -> PathBuf {
    let common = path
        .components()
        .zip(root.components())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel = PathBuf::new();
    for _ in root.components().skip(common) {
        rel.push("..");
    }
    rel.extend(path.components().skip(common));
    if rel.as_os_str().is_empty() {
        rel.push(".");
    }
    rel
}
fn packages(root: &Path, cargo: &str, env: &Env, args: &Args) -> Result<BTreeMap<PathBuf, Value>> {
    let mut cmd = isolated(root, env, cargo);
    cmd.args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
        "--filter-platform",
        &args.triple,
    ]);
    if args.no_default_features {
        cmd.arg("--no-default-features");
    }
    for f in &args.features {
        cmd.arg("--features").arg(if f.contains('/') {
            f.clone()
        } else {
            format!("{}/{f}", args.package)
        });
    }
    let meta: Value = serde_json::from_slice(&exec(&mut cmd)?.0)?;
    let mut result = BTreeMap::new();
    for row in arr(&meta["packages"])? {
        let manifest = Path::new(txt(&row["manifest_path"])?).canonicalize()?;
        let dir = manifest.parent().ok_or("missing package directory")?;
        let mut identity = json!({"name":row["name"],"version":row["version"]});
        if row["source"].is_null() {
            identity["path"] = json!(relative(dir, root));
        } else {
            identity["source"] = row["source"].clone();
        }
        result.insert(dir.to_owned(), identity);
    }
    Ok(result)
}
struct Capture {
    report: Value,
    #[cfg(test)]
    compiler: Value,
    #[cfg(test)]
    document: Document,
}
fn collect(args: &Args) -> Result<Capture> {
    let root = args.workspace.canonicalize()?;
    let revision = args.revision.as_deref();
    let source = pinned(&root, revision)?;
    let manifest = root.join("Cargo.toml");
    let name = args.target.replace('-', "_");
    let cargo = tool("cargo")?;
    let rustc = tool("rustc")?;
    let rustdoc = tool("rustdoc")?;
    let tools = tool_identity(&rustc, &rustdoc)?;
    let mut env: Env = [
        "HOME",
        "PATH",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "TMPDIR",
        "SDKROOT",
    ]
    .into_iter()
    .filter_map(|k| env::var(k).ok().map(|v| (k.into(), v)))
    .collect();
    env.insert("LC_ALL".into(), "C".into());
    let dir = tempfile::Builder::new()
        .prefix("oddlang-release-doc-")
        .tempdir()?;
    let run = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rust-recovery/run.sh");
    let (raw, log) = exec(
        isolated(&root, &env, run)
            .arg(&manifest)
            .args([&args.package, &args.kind, &args.target, &args.triple])
            .arg(if args.no_default_features {
                "no-default"
            } else {
                "default"
            })
            .args(&args.features)
            .env("ODD_RECOVERY_PROFILE_TRACE", "1"),
    )?;
    let compiler: Value = serde_json::from_slice(&raw)?;
    if compiler["compiler"] != COMMIT || compiler["crateName"] != name {
        return Err("wrong compiler inventory".into());
    }
    let packages = packages(&root, &cargo, &env, args)?;
    let compiler_args = profile::invocation(&log, &rustc, &name)?;
    let mut compiled = profile::profile(&compiler_args, &root);
    compiled["externBindings"] = profile::bindings(&log, &rustc, &compiler_args, &packages)?;

    let wrapper = dir.path().join("doc-wrapper");
    fs::write(
        &wrapper,
        "#!/bin/sh\nexport RUSTC_BOOTSTRAP=\"$ODD_DOC_CRATE\"\nexec \"$ODD_RUSTDOC\" \"$@\"\n",
    )?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    let remap = format!("--remap-path-prefix={}=/oddlang-workspace", root.display());
    let mut doc_env = env.clone();
    doc_env.extend([
        ("RUSTC".into(), rustc.clone()),
        ("RUSTDOC".into(), strpath(&wrapper)?.into()),
        ("ODD_RUSTDOC".into(), rustdoc),
        ("ODD_DOC_CRATE".into(), name.clone()),
        ("CARGO_ENCODED_RUSTFLAGS".into(), remap.clone()),
        ("CARGO_ENCODED_RUSTDOCFLAGS".into(), remap),
    ]);
    let build = dir.path().join("cargo");
    let mut flags = strings(&[
        "rustdoc",
        "--locked",
        "--offline",
        "--release",
        "--target",
        &args.triple,
        "--manifest-path",
        strpath(&manifest)?,
        "--target-dir",
        strpath(&build)?,
        "-p",
        &args.package,
        "-vv",
    ]);
    if args.kind == "lib" {
        flags.push("--lib".into());
    } else {
        flags.extend(strings(&["--bin", &args.target]));
    }
    if args.no_default_features {
        flags.push("--no-default-features".into());
    }
    for f in &args.features {
        flags.extend(strings(&["--features", f]));
    }
    flags.extend(strings(&[
        "--",
        "-Z",
        "unstable-options",
        "--output-format",
        "json",
        "--document-private-items",
    ]));
    let codegen: Vec<String> = serde_json::from_value(compiled["codegen"].clone())?;
    for setting in &codegen {
        flags.extend(strings(&["-C", setting]));
    }
    let (_, doc_log) = exec(isolated(&root, &doc_env, &cargo).args(&flags))?;
    let doc_args = profile::invocation(&doc_log, strpath(&wrapper)?, &name)?;
    let mut observed = profile::profile(&doc_args, &root);
    observed["externBindings"] = profile::bindings(&doc_log, &rustc, &doc_args, &packages)?;
    profile::compare_profiles(&compiled, &observed)?;
    if compiled["target"] != json!([args.triple])
        || compiled["pathMaps"] != json!(["--remap-path-prefix=$WORKSPACE=/oddlang-workspace"])
    {
        return Err("requested target or path map differs".into());
    }
    let path = build
        .join(&args.triple)
        .join("doc")
        .join(format!("{name}.json"));
    let document = Document::read(&path, &args.triple)?;
    let mut cfg = isolated(&root, &env, &rustc);
    cfg.args(["--print", "cfg", "--target", &args.triple]);
    for setting in &codegen {
        cfg.args(["-C", setting]);
    }
    for setting in arr(&compiled["explicitCfg"])? {
        cfg.args(["--cfg", txt(setting)?]);
    }
    let cfg_output = stdout(&mut cfg)?;
    let mut selected_cfg: Vec<_> = cfg_output.lines().collect();
    selected_cfg.sort();
    let compared = comparison::compare(&compiler, &document, &root)?;
    let prior = args.previous_spans.as_ref().map(|p| read(p)).transpose()?;
    let spans = spans::evidence(&compiler, &document, &root, prior.as_ref())?;
    let mut features = args.features.clone();
    features.sort();
    let mut report = json!({
        "schema":1,"source":source,"package":args.package,"targetKind":args.kind,"defaultFeatures":!args.no_default_features,
        "requestedFeatures":features,"tools":tools,"cargoProfile":"release","profile":compiled,"compilerCfg":selected_cfg,
        "rustdocImplicitCfg":["doc"],"cargoLockSha256":sha(&root.join("Cargo.lock"))?,"workspaceManifestSha256":sha(&manifest)?,
        "previousSpanReportSha256":args.previous_spans.as_ref().map(|p|sha(p)).transpose()?,"spanEvidence":spans,"formatVersion":57,"comparison":compared
    });
    if args.public_inherent {
        let (_, hidden_log) = exec(
            isolated(&root, &doc_env, &cargo)
                .args(&flags)
                .arg("--document-hidden-items"),
        )?;
        let hidden = profile::invocation(&hidden_log, strpath(&wrapper)?, &name)?;
        if hidden
            .iter()
            .filter(|s| *s == "--document-hidden-items")
            .count()
            != 1
            || !hidden
                .iter()
                .filter(|s| *s != "--document-hidden-items")
                .eq(doc_args.iter())
        {
            return Err("hidden-item observation changed the Rustdoc invocation".into());
        }
        let extended = Document::read(&path, &args.triple)?;
        let owners = comparison::owners(&extended)?;
        report["rustdocPublicInherent"] =
            provider_validation::inherent_members::inventory(&extended.into_index(), &owners)?;
    }
    if pinned(&root, revision)? != source {
        return Err("source changed during observation".into());
    }
    Ok(Capture {
        report,
        #[cfg(test)]
        compiler,
        #[cfg(test)]
        document,
    })
}
fn main() -> Result<()> {
    let args = Args::parse();
    let report = collect(&args)?.report;
    let mut contents = String::new();
    for c in serde_json::to_string_pretty(&report)?.chars() {
        if c < '\u{7f}' {
            contents.push(c);
        } else {
            for n in c.encode_utf16(&mut [0; 2]) {
                contents.push_str(&format!("\\u{n:04x}"));
            }
        }
    }
    contents.push('\n');
    fs::write(args.out, contents)?;
    let counts: BTreeMap<_, _> = ["matched", "compilerOnly", "rustdocOnly", "ambiguous"]
        .map(|k| (k, report["comparison"][k].as_array().unwrap().len()))
        .into();
    println!("{}", serde_json::to_string(&counts)?);
    Ok(())
}
#[cfg(test)]
mod tests;
