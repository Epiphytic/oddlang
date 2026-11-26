use oddlang::assemble::assemble;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};

const GENERATED: &str = "pub fn result() -> u32 { 42 }\n";
const LIB: &str =
    "[package]\nname='model'\nversion='0.1.0'\nedition='2024'\n[features]\nchecked=[]\n";
const APP: &str = "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nmodel={path='../model'}\n";
const LOCK: &str = "version = 4\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"model\"]\n[[package]]\nname = \"model\"\nversion = \"0.1.0\"\n";

fn sha(data: impl AsRef<[u8]>) -> String {
    Sha256::digest(data.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Fixture {
    root: tempfile::TempDir,
    manifest: PathBuf,
    store: PathBuf,
    candidate: PathBuf,
    projection: PathBuf,
}

impl Fixture {
    fn new(generated: &str, overrides: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("store");
        let candidate = root.path().join("candidate");
        let manifest = root.path().join("project.toml");
        let hash = sha(generated);
        let graph = sha(b"independent fixture graph");
        let receipt = serde_json::to_vec(&json!({"version":1,"graphSha256":graph,
                                                 "files":{"module.rs":hash}}))
        .unwrap();
        let projection_hash = sha(&receipt);
        let projection = store.join("projections").join(&projection_hash);
        fs::create_dir_all(&projection).unwrap();
        fs::write(projection.join("projection.json"), receipt).unwrap();
        fs::write(projection.join("module.rs"), generated).unwrap();
        fs::create_dir(store.join("retained")).unwrap();
        let mut retained = BTreeMap::from([
            (
                "Cargo.toml",
                "[workspace]\nmembers=['app','model']\nresolver='2'\n",
            ),
            ("Cargo.lock", LOCK),
            ("app/Cargo.toml", APP),
            (
                "app/src/main.rs",
                "fn main() { println!(\"{}\", model::result()); }\n",
            ),
            ("model/Cargo.toml", LIB),
            ("scripts/check", "#!/bin/sh\nexit 0\n"),
        ]);
        for &(name, content) in overrides {
            retained.insert(name, content);
        }
        let mut project = format!(
            "version=1\ncrates=['app','model']\ntarget='aarch64-apple-darwin'\nfeatures=['model/checked']\ngraphSha256='{graph}'\nprojectionSha256='{projection_hash}'\ncargoLockSha256='{}'\n",
            sha(LOCK)
        );
        for (name, content) in retained {
            let h = sha(content);
            fs::write(store.join("retained").join(&h), content).unwrap();
            let mode = if name == "scripts/check" {
                "100755"
            } else {
                "100644"
            };
            project.push_str(&format!(
                "\n[[files]]\npath='{name}'\nmode='{mode}'\nproducer='retained'\nsha256='{h}'\n"
            ));
        }
        project.push_str(&format!("\n[[files]]\npath='model/src/lib.rs'\nmode='100644'\nproducer='generated'\nmodule='module.rs'\nsha256='{hash}'\n"));
        fs::write(&manifest, project).unwrap();
        Self {
            root,
            manifest,
            store,
            candidate,
            projection,
        }
    }

    fn run(&self) -> Result<(), String> {
        assemble(&self.manifest, &self.store, &self.candidate)
    }

    fn rejected(&self, reason: &str) {
        let error = self.run().unwrap_err();
        assert!(error.contains(reason), "{error}");
        assert!(
            !self.candidate.exists() || fs::read_dir(&self.candidate).unwrap().next().is_none()
        );
    }

    fn change(&self, old: &str, new: &str) {
        let original = fs::read_to_string(&self.manifest).unwrap();
        assert!(original.contains(old));
        fs::write(&self.manifest, original.replace(old, new)).unwrap();
    }
}

#[test]
fn publishes_empty_destination_and_compiles_independent_workspace_consumer() {
    let f = Fixture::new(GENERATED, &[]);
    fs::create_dir(&f.candidate).unwrap();
    let assembled = Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .arg("assemble")
        .args([&f.manifest, &f.store, &f.candidate])
        .output()
        .unwrap();
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    assert_eq!(
        fs::read(f.candidate.join("model/src/lib.rs")).unwrap(),
        GENERATED.as_bytes()
    );
    assert_eq!(
        fs::metadata(f.candidate.join("scripts/check"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    // The candidate can build after the projection and retained input store disappear.
    fs::remove_dir_all(&f.store).unwrap();
    let output = Command::new("cargo")
        .args([
            "run",
            "--locked",
            "--offline",
            "-p",
            "app",
            "--features",
            "model/checked",
        ])
        .current_dir(&f.candidate)
        .env("CARGO_TARGET_DIR", f.root.path().join("target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn rejects_retained_fallback_symlinks_and_tampered_projection_without_publication() {
    let f = Fixture::new(GENERATED, &[]);
    let module = f.projection.join("module.rs");
    let blob = f.store.join("retained").join(sha(GENERATED));
    fs::rename(&module, &blob).unwrap();
    f.rejected("store entry");
    symlink(&blob, &module).unwrap();
    f.rejected("forbidden store entry");
    fs::remove_file(&module).unwrap();
    fs::rename(blob, &module).unwrap();
    fs::create_dir(&f.candidate).unwrap();
    fs::write(&module, "substituted").unwrap();
    f.rejected("digest mismatch");
    fs::write(f.projection.join("projection.json"), "{}").unwrap();
    f.rejected("digest mismatch");
}

#[test]
fn rejects_collisions_escapes_undeclared_inputs_and_stale_lock_or_graph() {
    for (old, new, error) in [
        (
            "path='app/src/main.rs'",
            "path='../main.rs'",
            "invalid portable",
        ),
        (
            "path='app/src/main.rs'",
            "path='App/src/main.rs'",
            "collision",
        ),
        (
            "path='app/src/main.rs'",
            "path='model/src/lib.rs'",
            "collision",
        ),
        (
            "path='app/src/main.rs'",
            "path='app/Cargo.toml/foo'",
            "collision",
        ),
        (
            "path='app/src/main.rs'",
            "path='.git/config'",
            "invalid portable",
        ),
        ("model/checked", "model/missing", "unknown selected feature"),
    ] {
        let f = Fixture::new(GENERATED, &[]);
        f.change(old, new);
        f.rejected(error);
    }
    for (field, expected) in [
        ("graphSha256", "projection disagrees"),
        ("cargoLockSha256", "missing root"),
    ] {
        let f = Fixture::new(GENERATED, &[]);
        let mut doc: toml::Value =
            toml::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        doc[field] = sha(b"substitution").into();
        fs::write(&f.manifest, toml::to_string(&doc).unwrap()).unwrap();
        f.rejected(expected);
    }
    let f = Fixture::new(GENERATED, &[]);
    fs::write(f.store.join("retained").join(sha(b"extra")), "extra").unwrap();
    f.rejected("store entry");
}

#[test]
fn rejects_include_and_build_script_substitutions_and_keeps_existing_work() {
    for (file, content) in [
        (
            "app/src/main.rs",
            "#[path=\"../../../outside.rs\"] mod outside;",
        ),
        ("app/src/main.rs", "include!(\"../../../outside.rs\");"),
        (
            "app/src/main.rs",
            "fn main(){println!(\"{}\",include_str!(\"../../../outside.rs\"));}",
        ),
        (
            "app/src/main.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/outside.rs\"));",
        ),
        (
            "model/Cargo.toml",
            "[package]\nname='model'\nversion='0.1.0'\nbuild='../../outside.rs'\n",
        ),
    ] {
        let f = Fixture::new(GENERATED, &[(file, content)]);
        fs::write(f.root.path().join("outside.rs"), "fn main() {}").unwrap();
        let err = f.run().unwrap_err();
        assert!(
            err.contains("escapes workspace") || err.contains("dynamic Rust include"),
            "{err}"
        );
        assert!(!f.candidate.exists());
    }
    let f = Fixture::new(
        "include!(\"../replacement.rs\");",
        &[("model/replacement.rs", GENERATED)],
    );
    f.rejected("emitted module");
    let f = Fixture::new(GENERATED, &[]);
    fs::create_dir(&f.candidate).unwrap();
    fs::write(f.candidate.join("owned"), b"kept").unwrap();
    assert!(f.run().unwrap_err().contains("empty directory"));
    assert_eq!(fs::read(f.candidate.join("owned")).unwrap(), b"kept");
}

#[test]
fn rejects_symlinked_inputs_and_output() {
    let f = Fixture::new(GENERATED, &[]);
    let faked = f.root.path().join("alias");
    symlink(&f.store, &faked).unwrap();
    assert!(assemble(&f.manifest, &faked, &f.candidate).is_err());
    let retained = f.store.join("retained");
    let external = f.root.path().join("external");
    fs::rename(&retained, &external).unwrap();
    symlink(&external, &retained).unwrap();
    f.rejected("forbidden store entry");
    fs::remove_file(&retained).unwrap();
    fs::rename(&external, &retained).unwrap();
    fs::create_dir(&external).unwrap();
    symlink(&external, &f.candidate).unwrap();
    assert!(f.run().unwrap_err().contains("empty directory"));
    assert!(fs::read_dir(&external).unwrap().next().is_none());
}
