//! Independently exercise checked values, fixed-width storage and field relations.
use provider_validation::{Result, cargo, command, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
mod checked;
mod fixed;
mod relation;

fn fixtures() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/checked-domains"
    ))
}
fn fixture(file: &str) -> Result<Value> {
    read(&fixtures().join(file))
}
fn component(
    root: &Path,
    kind: &str,
    module: &str,
    exports: &[&str],
    g: Value,
    r: Value,
) -> Result<Value> {
    let graph = format!("{module}.json");
    let rust = format!("{module}-rust.json");
    write(&root.join(&graph), &g)?;
    write(&root.join(&rust), &r)?;
    Ok(
        json!({"kind":kind,"module":module,"reexports":exports,"graph":pin(root,&graph)?,"overlay":pin(root,&rust)?}),
    )
}
fn build(
    cli: &Path,
    root: &Path,
    kind: &str,
    graph: &str,
    rust: &str,
    out: &str,
    error: Option<&str>,
) -> Result {
    invoke(cli, root, &["build", kind, "rust", graph, rust], out, error)
}
fn package(
    name: &str,
    version: &str,
    edition: &str,
    root: &Path,
    components: Value,
) -> Result<Value> {
    Ok(
        json!({"format":"oddlang-native-package-v1","name":name,"version":version,"edition":edition,"cargoLock":pin(root,"Cargo.lock")?,"components":components}),
    )
}
fn assemble(cli: &Path, root: &Path, spec: &Value, out: &str, error: Option<&str>) -> Result {
    write(&root.join("package.json"), spec)?;
    invoke(
        cli,
        root,
        &["build", "native-package", "rust", "package.json"],
        out,
        error,
    )
}
fn main() -> Result {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [case, exe] = args.as_slice() else {
        return Err(
            "usage: qualify-domains checked-values|field-relation|fixed-bytes ODDLANG".into(),
        );
    };
    let cli = Path::new(exe).canonicalize()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    match case.to_str() {
        Some("checked-values") => checked::verify(&cli, root),
        Some("field-relation") => relation::verify(&cli, root),
        Some("fixed-bytes") => fixed::verify(&cli, root),
        _ => Err("unknown domain qualification".into()),
    }
}
