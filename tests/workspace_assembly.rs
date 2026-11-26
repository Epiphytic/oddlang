use oddlang::assemble::assemble;
use oddlang::{framing_import, framing_projection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
    sync::OnceLock,
};

const SOURCE: &str = "examples/rust-import/framing-independent";
const FIXTURE_PATHS: [&str; 3] = ["import.lock", "source-map.json", "domain.ttl"];
const APP: &str = "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nmodel={package='independent-framing',path='../model'}\n";

#[derive(Clone, Debug)]
struct Imported {
    files: BTreeMap<String, String>,
    receipt: String,
    generated: String,
}

fn artifact() -> &'static Imported {
    static CACHE: OnceLock<Imported> = OnceLock::new();
    CACHE.get_or_init(|| {
        let scratch = tempfile::tempdir().unwrap();
        let semantic = scratch.path().join("semantic");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SOURCE);
        let copy = scratch.path().join("original");
        fs::create_dir_all(copy.join("src")).unwrap();
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "mapping.toml",
            "src/frame.rs",
            "src/lib.rs",
        ] {
            fs::copy(source.join(name), copy.join(name)).unwrap();
        }
        let source = copy;
        framing_import::import(
            &source.join("Cargo.toml"),
            &source.join("mapping.toml"),
            &semantic,
        )
        .unwrap();
        fs::remove_dir_all(source).unwrap();
        let files = FIXTURE_PATHS
            .into_iter()
            .map(|k| (k.to_owned(), fs::read_to_string(semantic.join(k)).unwrap()))
            .collect();
        let projection = scratch.path().join("projection");
        framing_projection::build(&semantic, &projection).unwrap();
        Imported {
            files,
            receipt: fs::read_to_string(projection.join("projection.json")).unwrap(),
            generated: fs::read_to_string(projection.join("frame.rs")).unwrap(),
        }
    })
}

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
        let graph =
            serde_json::from_slice::<Value>(artifact().receipt.as_bytes()).unwrap()["graphSha256"]
                .as_str()
                .unwrap()
                .to_owned();
        let receipt = if generated == &artifact().generated {
            artifact().receipt.clone()
        } else {
            let mut body: serde_json::Value = serde_json::from_str(&artifact().receipt).unwrap();
            body["files"]["frame.rs"] = serde_json::Value::String(hash.clone());
            serde_json::to_string_pretty(&body).unwrap() + "\n"
        };
        let projection_hash = sha(&receipt);
        let projection = store.join("projections").join(&projection_hash);
        fs::create_dir_all(&projection).unwrap();
        let import_hash = sha(&artifact().files["import.lock"]);
        let import = store.join("imports").join(&import_hash);
        fs::create_dir_all(&import).unwrap();
        for (name, content) in &artifact().files {
            fs::write(import.join(name), content).unwrap();
        }
        fs::write(projection.join("projection.json"), receipt).unwrap();
        fs::write(projection.join("frame.rs"), generated).unwrap();
        fs::create_dir(store.join("retained")).unwrap();
        let mut relative = toml::from_str::<toml::Value>(
            std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(SOURCE)
                    .join("Cargo.toml"),
            )
            .unwrap()
            .as_str(),
        )
        .unwrap();
        relative.as_table_mut().unwrap().remove("workspace");
        let model = toml::to_string(&relative).unwrap() + "\n[features]\nchecked=[]\n";
        let lock = fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(SOURCE)
                .join("Cargo.lock"),
        )
        .unwrap();
        let lock = lock
            + "\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"independent-framing\"]\n";
        let mut retained = BTreeMap::from([
            (
                "Cargo.toml",
                "[workspace]\nmembers=['app','model']\nresolver='2'\n",
            ),
            ("Cargo.lock", &*lock),
            ("app/Cargo.toml", APP),
            (
                "app/src/main.rs",
                "fn main() { println!(\"{}\", model::ProbeEnvelope::decode(&model::ProbeEnvelope::encode(&[42]).unwrap()).unwrap()[0]); }\n",
            ),
            ("model/Cargo.toml", &*model),
            ("scripts/check", "#!/bin/sh\nexit 0\n"),
        ]);
        for &(name, content) in overrides {
            retained.insert(name, content);
        }
        let mut project = format!(
            "version=1\ncrates=['app','independent-framing']\ntarget='aarch64-apple-darwin'\nfeatures=['independent-framing/checked']\nimportSha256='{import_hash}'\ngraphSha256='{graph}'\nprojectionSha256='{projection_hash}'\ncargoLockSha256='{}'\n",
            sha(&lock)
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
        project.push_str(&format!("\n[[files]]\npath='model/src/lib.rs'\nmode='100644'\nproducer='generated'\nmodule='frame.rs'\nsha256='{hash}'\n"));
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
    let f = Fixture::new(&artifact().generated, &[]);
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
        artifact().generated.as_bytes()
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
    let f = Fixture::new(&artifact().generated, &[]);
    let module = f.projection.join("frame.rs");
    let blob = f.store.join("retained").join(sha(&artifact().generated));
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
        (
            "independent-framing/checked",
            "independent-framing/absent",
            "unknown selected feature",
        ),
    ] {
        let f = Fixture::new(&artifact().generated, &[]);
        f.change(old, new);
        f.rejected(error);
    }
    for (field, expected) in [
        ("graphSha256", "projection disagrees"),
        ("cargoLockSha256", "missing root"),
    ] {
        let f = Fixture::new(&artifact().generated, &[]);
        let mut doc: toml::Value =
            toml::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        doc[field] = sha(b"substitution").into();
        fs::write(&f.manifest, toml::to_string(&doc).unwrap()).unwrap();
        f.rejected(expected);
    }
    let f = Fixture::new(&artifact().generated, &[]);
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
        let f = Fixture::new(&artifact().generated, &[(file, content)]);
        fs::write(f.root.path().join("outside.rs"), "fn main() {}").unwrap();
        let err = f.run().unwrap_err();
        assert!(
            err.contains("escapes workspace") || err.contains("dynamic Rust include"),
            "{err}"
        );
        assert!(!f.candidate.exists());
    }
    let f = Fixture::new(&artifact().generated, &[]);
    fs::create_dir(&f.candidate).unwrap();
    fs::write(f.candidate.join("owned"), b"kept").unwrap();
    assert!(f.run().unwrap_err().contains("empty directory"));
    assert_eq!(fs::read(f.candidate.join("owned")).unwrap(), b"kept");
}

#[test]
fn rejects_symlinked_inputs_and_output() {
    let f = Fixture::new(&artifact().generated, &[]);
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

#[test]
fn anchored_store_rejects_coherently_rehashed_projection() {
    let fixture = Fixture::new(&format!("// altered bytes\n{}", artifact().generated), &[]);
    let cached: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.projection.join("projection.json")).unwrap())
            .unwrap();
    let bytes = fs::read_to_string(fixture.projection.join("projection.json")).unwrap();
    let approved: Value = serde_json::from_str(&artifact().receipt).unwrap();
    assert_eq!(
        cached["files"]["frame.rs"],
        sha(fs::read(fixture.projection.join("frame.rs")).unwrap())
    );
    assert_eq!(cached["graphSha256"], approved["graphSha256"]);
    assert_eq!(
        fixture.projection.file_name().unwrap().to_str().unwrap(),
        sha(&bytes)
    );
    assert_ne!(cached["files"]["frame.rs"], approved["files"]["frame.rs"]);
    fixture.rejected("projection receipt differs from the isolated emitter");
}

#[test]
fn independently_pinned_release_digest_is_required() {
    let data = artifact();
    let lock = &data.files["import.lock"];
    let f = Fixture::new(&data.generated, &[]);
    let expected = f.store.join("imports").join(sha(lock));
    fs::write(expected.join("import.lock"), lock.to_owned() + "\n").unwrap();
    assert_eq!(f.run().unwrap_err(), "external framing import pin mismatch");
    assert!(!f.candidate.exists());
}

fn facade_artifact() -> &'static Imported {
    static CACHE: OnceLock<Imported> = OnceLock::new();
    CACHE.get_or_init(|| {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("original");
        fs::create_dir_all(src.join("src")).unwrap();
        for (name, text) in [
            (
                "Cargo.toml",
                "[package]\nname='app'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
            ),
            (
                "Cargo.lock",
                "version=4\n[[package]]\nname='app'\nversion='0.1.0'\n",
            ),
            (
                "facade.toml",
                "profile='rust-facade-v1'\npackage='app'\n[exports]\nticket=['Ticket']\n",
            ),
            ("src/lib.rs", "mod ticket;\npub use ticket::Ticket;\n"),
            ("src/ticket.rs", "pub struct Ticket;\n"),
        ] {
            fs::write(src.join(name), text).unwrap();
        }
        let semantic = temp.path().join("semantic");
        oddlang::facade::import(&src.join("Cargo.toml"), &src.join("facade.toml"), &semantic)
            .unwrap();
        fs::remove_dir_all(src).unwrap();
        let files: BTreeMap<_, _> = FIXTURE_PATHS
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    fs::read_to_string(semantic.join(name)).unwrap(),
                )
            })
            .collect();
        let output = temp.path().join("output");
        oddlang::facade::build_pinned(&semantic, &sha(&files["import.lock"]), &output).unwrap();
        Imported {
            files,
            receipt: fs::read_to_string(output.join("projection.json")).unwrap(),
            generated: fs::read_to_string(output.join("lib.rs")).unwrap(),
        }
    })
}

fn composite() -> Fixture {
    let facade = facade_artifact();
    let f = Fixture::new(
        &artifact().generated,
        &[
            (
                "model/src/lib.rs",
                "pub mod frame;\npub use frame::ProbeEnvelope;\n",
            ),
            ("app/src/ticket.rs", "pub struct Ticket;\n"),
            (
                "app/src/main.rs",
                "fn main(){let _ = app::Ticket; println!(\"{}\", model::ProbeEnvelope::decode(&model::ProbeEnvelope::encode(&[42]).unwrap()).unwrap()[0]);}\n",
            ),
        ],
    );
    let mut doc: toml::Table = toml::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
    doc.insert("version".into(), 2.into());
    let mut frame = toml::Table::new();
    for key in ["graphSha256", "projectionSha256", "importSha256"] {
        frame.insert(key.to_owned(), doc.remove(key).unwrap());
    }
    let target: Value = serde_json::from_str(&artifact().files["import.lock"]).unwrap();
    doc.insert(
        "target".into(),
        target["build"]["target"].as_str().unwrap().into(),
    );
    frame.insert("profile".into(), "framing-v1".into());
    frame.insert("package".into(), "independent-framing".into());
    let receipt: Value = serde_json::from_str(&facade.receipt).unwrap();
    let release = sha(&facade.files["import.lock"]);
    let projection_hash = sha(&facade.receipt);
    let import = f.store.join("imports").join(&release);
    fs::create_dir(&import).unwrap();
    for (name, bytes) in &facade.files {
        fs::write(import.join(name), bytes).unwrap();
    }
    let projection = f.store.join("projections").join(&projection_hash);
    fs::create_dir(&projection).unwrap();
    fs::write(projection.join("projection.json"), &facade.receipt).unwrap();
    fs::write(projection.join("lib.rs"), &facade.generated).unwrap();
    let second: toml::Value = toml::from_str(&format!("profile='rust-facade-v1'\npackage='app'\ngraphSha256='{}'\nimportSha256='{release}'\nprojectionSha256='{projection_hash}'",receipt["graphSha256"].as_str().unwrap())).unwrap();
    doc.insert(
        "emitters".into(),
        toml::Value::Array(vec![toml::Value::Table(frame), second]),
    );
    let rows = doc.get_mut("files").unwrap().as_array_mut().unwrap();
    let row = rows
        .iter_mut()
        .find(|r| r["producer"].as_str() == Some("generated"))
        .unwrap()
        .as_table_mut()
        .unwrap();
    row.insert("path".into(), "model/src/frame.rs".into());
    row.insert("emitter".into(), "framing-v1".into());
    rows.push(toml::from_str(&format!("path='app/src/lib.rs'\nproducer='generated'\nemitter='rust-facade-v1'\nmodule='lib.rs'\nmode='100644'\nsha256='{}'",sha(&facade.generated))).unwrap());
    fs::write(&f.manifest, toml::to_string(&doc).unwrap()).unwrap();
    f
}

#[test]
fn two_independent_emitters_compile_with_original_inputs_removed() {
    let f = composite();
    f.run().unwrap();
    fs::remove_dir_all(&f.store).unwrap();
    let run = Command::new("cargo")
        .args(["run", "--locked", "--offline", "-p", "app"])
        .current_dir(&f.candidate)
        .env("CARGO_TARGET_DIR", f.root.path().join("target"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"42\n");
}

#[test]
fn second_emitter_rejects_swaps_extra_blobs_and_coherently_rehashed_native_module() {
    let f = composite();
    let project = fs::read_to_string(&f.manifest).unwrap();
    f.change("emitter = \"rust-facade-v1\"", "emitter = \"framing-v1\"");
    f.rejected("unverified or reused emitted module");
    fs::write(&f.manifest, &project).unwrap();
    let extra = f.store.join("retained").join(sha("undeclared"));
    fs::write(&extra, "undeclared").unwrap();
    f.rejected("undeclared store entry");
    fs::remove_file(extra).unwrap();
    let facade = facade_artifact();
    let old_pin = sha(&facade.receipt);
    let projection = f.store.join("projections").join(&old_pin);
    let changed = facade.generated.clone() + "// changed\n";
    let receipt = facade
        .receipt
        .replace(&sha(&facade.generated), &sha(&changed));
    fs::write(projection.join("lib.rs"), &changed).unwrap();
    fs::write(projection.join("projection.json"), &receipt).unwrap();
    fs::rename(&projection, projection.with_file_name(sha(&receipt))).unwrap();
    f.change(&old_pin, &sha(&receipt));
    f.change(&sha(&facade.generated), &sha(&changed));
    f.rejected("projection receipt differs from the isolated emitter");
}

#[test]
fn second_import_rejects_coherent_graph_and_source_map_rewrite_under_original_release_pin() {
    let f = composite();
    let original = &facade_artifact().files;
    let dir = f.store.join("imports").join(sha(&original["import.lock"]));
    let graph = original["domain.ttl"].clone() + "\n";
    let old_graph = sha(&original["domain.ttl"]);
    let map = original["source-map.json"].replace(&old_graph, &sha(&graph));
    let lock = original["import.lock"]
        .replace(&old_graph, &sha(&graph))
        .replace(&sha(&original["source-map.json"]), &sha(&map));
    assert_ne!(map, original["source-map.json"]);
    assert_ne!(lock, original["import.lock"]);
    fs::write(dir.join("domain.ttl"), graph).unwrap();
    fs::write(dir.join("source-map.json"), map).unwrap();
    fs::write(dir.join("import.lock"), lock).unwrap();
    f.rejected("external facade import pin mismatch");
}
