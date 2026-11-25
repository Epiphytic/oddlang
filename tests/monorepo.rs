#[path = "support/db.rs"]
mod db;
#[path = "support/events.rs"]
mod events;
#[path = "../examples/contracts/monorepo/project.rs"]
mod project;

use oddlang::contracts::{Release, resolve};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn run(cmd: &mut Command) {
    let result = cmd.output().unwrap();
    assert!(
        result.status.success(),
        "{cmd:?}\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for f in fs::read_dir(src).unwrap() {
        let f = f.unwrap();
        let to = dest.join(f.file_name());
        if f.file_type().unwrap().is_dir() {
            copy_tree(&f.path(), &to);
        } else {
            fs::copy(f.path(), to).unwrap();
        }
    }
}
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|f| {
            let f = f.unwrap();
            (PathBuf::from(f.file_name()), fs::read(f.path()).unwrap())
        })
        .collect()
}
fn compile(work: &Path) -> BTreeMap<&'static str, Vec<u8>> {
    use sha2::{Digest, Sha256};
    run(Command::new("cargo")
        .current_dir(work.join("native"))
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", work.join("native/target"))
        .args(["build", "--locked", "--offline"]));
    run(Command::new("npm").current_dir(work).args([
        "exec",
        "--offline",
        "--package=typescript@5.9.3",
        "--",
        "tsc",
        "-p",
        "tsconfig.json",
    ]));
    [
        "native/target/debug/service",
        "native/target/debug/ledger-cli",
        "dist/wire.js",
        "dist/generated/contract.js",
    ]
    .into_iter()
    .map(|p| (p, Sha256::digest(fs::read(work.join(p)).unwrap()).to_vec()))
    .collect()
}
fn pin(root: &Path, work: &Path, version: &str) -> Release {
    fs::copy(
        root.join(format!("contracts/workspace-{version}.lock.json")),
        work.join("workspace.lock.json"),
    )
    .unwrap();
    resolve(
        &work.join("project.ttl"),
        &work.join("workspace.lock.json"),
        &root.join("contracts/cache"),
    )
    .unwrap()
}
#[test]
fn coordinated_workspace_pin_only_upgrade() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path();
    copy_tree(&root.join("examples/contracts/monorepo"), work);
    let ontology = fs::read(work.join("project.ttl")).unwrap();
    let generated = work.join("generated");
    let mut releases = Vec::new();
    let mut snapshots = Vec::new();
    let native = work.join("native");
    for version in ["v1", "v2"] {
        let release = pin(root, work, version);
        project::generate(&release, &generated).unwrap();
        let bytes = snapshot(&generated);
        // The migration witness executes these generators on the same resolved models.
        assert_eq!(
            bytes[Path::new("schema.sql")],
            oddlang::targets::postgres::generate(&release.model).as_bytes()
        );
        assert_eq!(
            bytes[Path::new("account.avsc")],
            oddlang::targets::avro::generate(&release.model).as_bytes()
        );
        let compiled = compile(work);
        fs::remove_dir_all(native.join("target")).unwrap();
        fs::remove_dir_all(work.join("dist")).unwrap();
        fs::remove_dir_all(&generated).unwrap();
        let replay = resolve(
            &work.join("project.ttl"),
            &work.join("workspace.lock.json"),
            &root.join("contracts/cache"),
        )
        .unwrap();
        project::generate(&replay, &generated).unwrap();
        assert_eq!(snapshot(&generated), bytes);
        let rebuilt = compile(work);
        assert_eq!(rebuilt, compiled, "offline compiled artifacts changed");
        let service = native.join("target/debug/service");
        let cli = native.join("target/debug/ledger-cli");
        run(Command::new("node")
            .current_dir(work)
            .arg("verify.cjs")
            .arg(service)
            .arg(cli));
        assert_eq!(fs::read(work.join("project.ttl")).unwrap(), ontology);
        snapshots.push(bytes);
        releases.push(release);
    }
    let changed: Vec<_> = snapshots[0]
        .keys()
        .filter(|name| snapshots[0][*name] != snapshots[1][*name])
        .filter_map(|p| p.to_str())
        .collect();
    assert_eq!(
        changed,
        [
            "account.avsc",
            "account.rs",
            "adapter.rs",
            "contract.ts",
            "schema.sql",
            "vectors.json"
        ]
    );
    assert!(snapshots[0].keys().eq(snapshots[1].keys()));
    let mut database = db::TestDatabase::connect().unwrap();
    let evidence =
        events::evolution_witness(&mut database.client, &releases[0].model, &releases[1].model);
    assert_eq!(evidence.historical.currency_code(), "CAD");
    assert_eq!(evidence.current.currency_code(), "JPY");
    assert!(evidence.missing_default.contains("missing reader default"));
}
