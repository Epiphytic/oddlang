//! Independently qualify Rust export graphs with rustdoc and downstream rustc.
use provider_validation::{Result, read, sha};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
mod adapter;
mod compare;
mod downstream;
mod observer;
mod semantics;
mod shapes;
mod tests;
const PIN: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
type Spelling = Vec<String>;
type Edges = BTreeMap<Spelling, Value>;
fn object(v: &Value) -> Result<&serde_json::Map<String, Value>> {
    v.as_object().ok_or_else(|| "expected JSON object".into())
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| "expected JSON array".into())
}
fn string(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(|| "expected JSON string".into())
}
fn paths(v: &Value) -> Result<Spelling> {
    Ok(serde_json::from_value(v.clone())?)
}
fn write(path: &Path, v: &Value) -> Result {
    Ok(fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(v)?),
    )?)
}
fn bindings(g: &Value, table: &str) -> Result<Edges> {
    let mut edges = Edges::new();
    for row in object(&g["modules"])?.values() {
        for (name, edge) in object(&row[table])? {
            let mut path = paths(&row["path"])?;
            path.push(name.clone());
            edges.insert(path, edge.clone());
        }
    }
    Ok(edges)
}
fn cmd(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(exe);
    for (k, _) in env::vars() {
        if [
            "RUST",
            "CARGO_ENCODED_",
            "CARGO_PROFILE_",
            "CARGO_BUILD_RUST",
            "CARGO_TARGET_",
        ]
        .iter()
        .any(|prefix| k.starts_with(prefix))
            || k == "CARGO_BUILD_TARGET"
        {
            c.env_remove(k);
        }
    }
    c.env("RUSTUP_TOOLCHAIN", "1.97.1")
        .env("CARGO_NET_OFFLINE", "true");
    c
}
fn capture(command: &mut Command) -> Result<Output> {
    provider_validation::subprocess::run_with_timeout(
        command,
        None,
        std::time::Duration::from_secs(1800),
    )
}
fn checked(command: &mut Command) -> Result<Vec<u8>> {
    let out = capture(command)?;
    if !out.status.success() {
        return Err(format!("{command:?}: {}", String::from_utf8_lossy(&out.stderr)).into());
    }
    Ok(out.stdout)
}
fn installed_adapter() -> Result<PathBuf> {
    let bytes = checked(provider_validation::capture_command().arg("--install-adapter"))?;
    let path = PathBuf::from(std::str::from_utf8(&bytes)?.trim_end_matches(['\r', '\n']));
    if !path.is_absolute() || !path.is_file() {
        return Err("native compiler adapter installation did not produce an executable".into());
    }
    Ok(path)
}
fn version() -> Result<String> {
    Ok(String::from_utf8(checked(cmd("rustc").arg("-vV"))?)?)
}
fn absolute(p: &str) -> Result<PathBuf> {
    Ok(std::path::absolute(p)?)
}
fn spelling(path: &[String]) -> String {
    path.iter()
        .map(|s| {
            if matches!(s.as_str(), "crate" | "self" | "super" | "Self") {
                s.clone()
            } else {
                format!("r#{s}")
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}
fn metadata(manifest: &Path, cwd: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&checked(
        cmd("cargo")
            .current_dir(cwd)
            .args([
                "metadata",
                "--locked",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(manifest),
    )?)?)
}
fn compile(work: &Path, dep: &Path, source: &str) -> Result<Output> {
    fs::write(work.join("caller.rs"), source)?;
    let target = dep
        .ancestors()
        .nth(3)
        .ok_or("invalid native artifact path")?;
    capture(
        cmd("rustc")
            .current_dir(work)
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "lib",
                "--emit=metadata",
                "--target",
            ])
            .arg(target.file_name().ok_or("missing triple")?)
            .args([
                "--error-format=json",
                "--crate-name",
                "separate_caller",
                "--out-dir",
            ])
            .arg(work)
            .arg("--extern")
            .arg(format!("observed={}", dep.display()))
            .arg("-L")
            .arg(format!("dependency={}", dep.parent().unwrap().display()))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                target.parent().unwrap().join("release/deps").display()
            ))
            .arg("caller.rs"),
    )
}
fn require(out: Output) -> Result {
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(())
}
fn diagnostics(out: &Output) -> Result<Vec<Value>> {
    Ok(std::str::from_utf8(&out.stderr)?
        .lines()
        .filter(|l| l.starts_with('{'))
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?)
}
fn imports(name: &[String]) -> String {
    format!("pub use r#observed::{} as witness;", spelling(name))
}
fn unresolved(g: &Value) -> Result<Vec<Spelling>> {
    let mut out = Vec::new();
    for row in object(&g["modules"])?.values() {
        for name in paths(&row["unresolved"])? {
            let mut p = paths(&row["path"])?;
            p.push(name.clone());
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}
fn main() -> Result {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice(){
        [op,case] if op=="qualify" && case=="adapter"=>adapter::verify(tempfile::tempdir()?.path()),
        [op,case,cli] if op=="qualify"=>tests::run(case,Path::new(cli)),
        [op,rest @ ..] if op=="probe"=>observer::probe(rest),
        [op,reference,subject,manifest,cache,out] if op=="consumer-paths"=>write(Path::new(out),&downstream::probe(Path::new(reference),Path::new(subject),&Path::new(manifest).canonicalize()?,&absolute(cache)?)?),
        [op,old,new,out] if op=="compare"=>compare::report(Path::new(old),Path::new(new),Path::new(out)),
        [op,compiler,doc] if op=="reconcile"=>compare::reconcile(Path::new(compiler),Path::new(doc)),
        _=>Err("usage: rust-exports probe ODDLANG MANIFEST PACKAGE OUT [OPTIONS] | consumer-paths EXPECTED SUBJECT MANIFEST CACHE RECEIPT | compare ORIGINAL GENERATED OUT | reconcile COMPILER_INVENTORY RUSTDOC_INVENTORY".into())
    }
}
