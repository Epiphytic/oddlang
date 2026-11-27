use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const PACKAGE: &str = "rust-tree-control";
const FILES: [&str; 8] = [
    "build.rs",
    "src/basic.rs",
    "src/bin/shebang.rs",
    "src/bom.rs",
    "src/crlf.rs",
    "src/extra.rs",
    "src/lib.rs",
    "tests/observations.rs",
];
const ASSETS: [&str; 3] = ["Cargo.toml", "Cargo.lock", "inputs/delta.txt"];
const BUNDLE_FILES: [&str; 3] = ["bundle.lock", "sources.json", "syntax.ttl"];
const BASIC_SUBJECT: &str = "<urn:oddlang:rust-tree:file:src/basic.rs";
const SPELLING: &str = " <urn:oddlang:rust-tree:spelling> ";

#[derive(Deserialize, Serialize)]
struct Inventory {
    version: Value,
    package: String,
    edition: Value,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    version: Value,
    syntax_sha256: String,
    sources_sha256: String,
}

struct Control {
    scratch: TempDir,
    source: PathBuf,
}

impl Control {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rust-tree-control");
        let source = scratch.path().join("capture-input");
        for file in FILES.into_iter().chain(ASSETS) {
            copy(&fixture.join(file), &source.join(file));
        }
        Self { scratch, source }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.scratch.path().join(name)
    }

    fn capture(&self, name: &str, package: &str) -> PathBuf {
        let bundle = self.path(name);
        ok(cli()
            .args(["import", "rust-tree"])
            .arg(self.source.join("Cargo.toml"))
            .arg(package)
            .arg(&bundle)
            .output()
            .unwrap());
        bundle
    }

    fn withdraw(&self) -> BTreeMap<&'static str, Vec<u8>> {
        let contents = FILES
            .into_iter()
            .map(|file| (file, fs::read(self.source.join(file)).unwrap()))
            .collect();
        fs::remove_dir_all(self.source.join("src")).unwrap();
        fs::remove_dir_all(self.source.join("tests")).unwrap();
        fs::remove_file(self.source.join("build.rs")).unwrap();
        contents
    }

    fn include_assets(&self, root: &Path) {
        for file in ASSETS {
            copy(&self.source.join(file), &root.join(file));
        }
    }
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
}

fn ok(output: Output) -> Output {
    assert!(
        output.status.success(),
        "command exited {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn copy(input: &Path, output: &Path) {
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::copy(input, output).unwrap();
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_json(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn canonical_graph(lines: impl IntoIterator<Item = String>) -> String {
    let mut lines: Vec<_> = lines.into_iter().collect();
    lines.sort_unstable();
    lines.join("\n") + "\n"
}

fn pin(bundle: &Path) -> String {
    sha(&fs::read(bundle.join("bundle.lock")).unwrap())
}

fn update_bundle(bundle: &Path, graph: &str, inventory: &Inventory) -> String {
    let mut lock: Lock =
        serde_json::from_slice(&fs::read(bundle.join("bundle.lock")).unwrap()).unwrap();
    let inventory = canonical_json(inventory);
    lock.syntax_sha256 = sha(graph.as_bytes());
    lock.sources_sha256 = sha(&inventory);
    let lock = canonical_json(&lock);
    fs::write(bundle.join("syntax.ttl"), graph).unwrap();
    fs::write(bundle.join("sources.json"), inventory).unwrap();
    fs::write(bundle.join("bundle.lock"), &lock).unwrap();
    sha(&lock)
}

fn build(bundle: &Path, destination: &Path, pin: &str) -> Output {
    cli()
        .args(["build", "rust-tree"])
        .arg(bundle)
        .arg(destination)
        .args(["--bundle-sha256", pin])
        .output()
        .unwrap()
}

fn reject(bundle: &Path, destination: &Path, pin: &str) {
    let output = build(bundle, destination, pin);
    assert!(
        !output.status.success(),
        "accepted corrupted bundle at {}",
        bundle.display()
    );
    assert!(!destination.exists(), "failed build published an output");
}

fn inventory(bundle: &Path) -> Inventory {
    serde_json::from_slice(&fs::read(bundle.join("sources.json")).unwrap()).unwrap()
}

fn change_basic_leaf(graph: &str, spelling: &str) -> String {
    let needle = format!("{SPELLING}\"7\" .");
    let mut changed = 0;
    let result = canonical_graph(graph.lines().map(|line| {
        if line.starts_with(BASIC_SUBJECT) && line.ends_with(&needle) {
            changed += 1;
            line.replace(&needle, &format!("{SPELLING}\"{spelling}\" ."))
        } else {
            line.to_owned()
        }
    }));
    assert_eq!(changed, 1, "expected one structured integer leaf");
    result
}

#[test]
fn capture_excludes_configured_cargo_outputs_and_nested_packages() {
    for target in ["target", "local-cache"] {
        let control = Control::new();
        let root = control.source.canonicalize().unwrap();
        for path in [target, "build-cache", "embedded"] {
            let dir = root.join(path);
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("old.rs"), "invalid as standalone Rust").unwrap();
        }
        fs::write(
            root.join("embedded/Cargo.toml"),
            "[package]\nname = \"embedded\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let source = b"const EXTRA: u8 = 2;\n";
        fs::create_dir(root.join("src/inactive")).unwrap();
        fs::write(root.join("src/inactive/extra.rs"), source).unwrap();

        let bundle = control.path("bundle");
        ok(cli()
            .args(["import", "rust-tree"])
            .arg(root.join("Cargo.toml"))
            .arg(PACKAGE)
            .arg(&bundle)
            .env("CARGO_TARGET_DIR", root.join(target))
            .env("CARGO_BUILD_BUILD_DIR", root.join("build-cache"))
            .output()
            .unwrap());
        let captured = inventory(&bundle);
        assert_eq!(captured.edition, "2024");
        let expected = FILES
            .into_iter()
            .map(|file| (file.to_owned(), sha(&fs::read(root.join(file)).unwrap())))
            .chain([("src/inactive/extra.rs".into(), sha(source))])
            .collect::<BTreeMap<_, _>>();
        assert_eq!(captured.files, expected);
    }
}

#[test]
fn regenerates_all_source_bytes_from_a_reordered_pinned_graph() {
    let control = Control::new();
    let first = control.capture("first-capture", PACKAGE);
    let second = control.capture("second-capture", PACKAGE);
    for file in BUNDLE_FILES {
        assert_eq!(
            fs::read(first.join(file)).unwrap(),
            fs::read(second.join(file)).unwrap(),
            "independent captures disagree for {file}"
        );
    }
    assert_eq!(
        inventory(&first)
            .files
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        FILES,
        "bundle inventory does not cover this package's complete frozen source list"
    );
    let expected = control.withdraw();
    let canonical = fs::read_to_string(second.join("syntax.ttl")).unwrap();
    let mut reversed = canonical.lines().rev().collect::<Vec<_>>().join("\n");
    reversed.push('\n');
    assert_ne!(reversed, canonical);
    fs::write(second.join("syntax.ttl"), reversed).unwrap();
    for (bundle, name) in [(&first, "first-emission"), (&second, "second-emission")] {
        let destination = control.path(name);
        ok(build(bundle, &destination, &pin(&first)));
        for (file, bytes) in &expected {
            assert_eq!(fs::read(destination.join(file)).unwrap(), *bytes, "{file}");
        }
    }
}

#[test]
fn rejects_stale_pins_and_corrupt_reauthorized_bundles() {
    let control = Control::new();
    let bundle = control.capture("bundle", PACKAGE);
    let original_inventory = inventory(&bundle);
    let original_graph = fs::read_to_string(bundle.join("syntax.ttl")).unwrap();
    let original_pin = pin(&bundle);
    let destination = control.path("must-not-exist");

    reject(&bundle, &destination, &"0".repeat(64));

    let mut invalid_path = inventory(&bundle);
    let valid_digest = invalid_path.files.remove("src/basic.rs").unwrap();
    invalid_path
        .files
        .insert("../escaped.rs".to_owned(), valid_digest);
    let invalid_graph = canonical_graph(original_graph.lines().map(|line| {
        line.replace(BASIC_SUBJECT, "<urn:oddlang:rust-tree:file:../escaped.rs")
            .replace("\"src/basic.rs\"", "\"../escaped.rs\"")
    }));
    let invalid_pin = update_bundle(&bundle, &invalid_graph, &invalid_path);
    reject(&bundle, &destination, &invalid_pin);
    assert!(!control.path("escaped.rs").exists());

    let without_file = canonical_graph(
        original_graph
            .lines()
            .filter(|line| !line.starts_with(BASIC_SUBJECT))
            .map(str::to_owned),
    );
    assert_ne!(without_file, original_graph);
    let omitted_pin = update_bundle(&bundle, &without_file, &original_inventory);
    reject(&bundle, &destination, &omitted_pin);

    let mut next_version = inventory(&bundle);
    next_version.version = serde_json::json!(2);
    let invalid_pin = update_bundle(&bundle, &original_graph, &next_version);
    reject(&bundle, &destination, &invalid_pin);

    let root = "<urn:oddlang:rust-tree:file:src/basic.rs#0>";
    let root_edge = format!("{root} <urn:oddlang:rust-tree:child:0> ");
    let cyclic = canonical_graph(original_graph.lines().map(|line| {
        if line.starts_with(&root_edge) {
            format!("{root_edge}{root} .")
        } else {
            line.to_owned()
        }
    }));
    assert_ne!(cyclic, original_graph);
    let invalid_pin = update_bundle(&bundle, &cyclic, &original_inventory);
    reject(&bundle, &destination, &invalid_pin);

    let mut invalid_leaf = original_inventory;
    invalid_leaf.files.insert(
        "src/basic.rs".to_owned(),
        sha(b"pub(super) const DELTA: i32 = 7 + 0;\n"),
    );
    let invalid_graph = change_basic_leaf(&original_graph, "7 + 0");
    let invalid_pin = update_bundle(&bundle, &invalid_graph, &invalid_leaf);
    assert_ne!(invalid_pin, original_pin);
    reject(&bundle, &destination, &invalid_pin);
}

fn cargo() -> Command {
    let mut command = Command::new("cargo");
    command.arg("+1.97.1");
    command
}

fn control_test(project: &Path, target: &Path, extra: bool) -> Output {
    let mut command = cargo();
    command
        .current_dir(project)
        .env("CARGO_TARGET_DIR", target)
        .args(["test", "--locked", "--offline"]);
    if extra {
        command.args(["--features", "extra"]);
    }
    command.output().unwrap()
}

#[test]
#[ignore = "runs six clean Cargo builds under pinned Rust 1.97.1"]
fn generated_crates_follow_the_independent_native_observations() {
    let control = Control::new();
    let bundle = control.capture("bundle", PACKAGE);
    let renamed = Control::new();
    let manifest = renamed.source.join("Cargo.toml");
    let revised = fs::read_to_string(&manifest)
        .unwrap()
        .replace(PACKAGE, "renamed-contract-probe")
        .replace("roundtrip_proc", "renamed_builder");
    fs::write(manifest, revised).unwrap();
    let source = renamed.source.join("src/lib.rs");
    let revised = fs::read_to_string(&source)
        .unwrap()
        .replace("roundtrip_proc", "renamed_builder");
    fs::write(source, revised).unwrap();
    ok(cargo()
        .current_dir(&renamed.source)
        .args(["generate-lockfile", "--offline"])
        .output()
        .unwrap());
    let renamed_bundle = renamed.capture("renamed-bundle", "renamed-contract-probe");

    for (fixture, bundle) in [(&control, &bundle), (&renamed, &renamed_bundle)] {
        let expected = fixture.withdraw();
        let candidate = fixture.path("generated");
        ok(build(bundle, &candidate, &pin(bundle)));
        for (file, bytes) in &expected {
            assert_eq!(fs::read(candidate.join(file)).unwrap(), *bytes, "{file}");
        }
        fixture.include_assets(&candidate);
        ok(control_test(
            &candidate,
            &fixture.path("default-target"),
            false,
        ));
        ok(control_test(
            &candidate,
            &fixture.path("extra-target"),
            true,
        ));
    }

    let graph = fs::read_to_string(bundle.join("syntax.ttl")).unwrap();
    let mut files = inventory(&bundle);
    files.files.insert(
        "src/basic.rs".to_owned(),
        sha(b"pub(super) const DELTA: i32 = 8;\n"),
    );
    let graph = change_basic_leaf(&graph, "8");
    let edited_pin = update_bundle(&bundle, &graph, &files);
    let edited = control.path("edited");
    ok(build(&bundle, &edited, &edited_pin));
    let unmodified = control.path("generated");
    for file in FILES.into_iter().filter(|file| *file != "src/basic.rs") {
        assert_eq!(
            fs::read(edited.join(file)).unwrap(),
            fs::read(unmodified.join(file)).unwrap(),
            "edit unexpectedly changed {file}"
        );
    }
    control.include_assets(&edited);
    let changed = control_test(&edited, &control.path("mutation-default-target"), false);
    assert!(
        !changed.status.success(),
        "the unchanged oracle missed the edit"
    );
    let log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&changed.stdout),
        String::from_utf8_lossy(&changed.stderr),
    );
    assert!(
        log.contains("left: Observation { result: 30,")
            && log.contains("right: Observation { result: 29,"),
        "the graph edit failed without the intended observed difference:\n{log}"
    );
    ok(control_test(
        &edited,
        &control.path("mutation-extra-target"),
        true,
    ));
}
