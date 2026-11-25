use serde_json::{Value, json};
use std::{fs, path::Path};

#[derive(Clone, Copy)]
enum Edit {
    Original,
    Label,
    Rename,
    SwapTags,
    RequiredImport,
}

/// Exercise publication and comparison through their public command line entry points.
fn publish(root: &Path, edit: Edit) {
    let fixture = Path::new("contracts/wasm/echo");
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::copy(
        fixture.join("dependencies.json"),
        source.join("dependencies.json"),
    )
    .unwrap();
    let mut turtle = fs::read_to_string(fixture.join("model.ttl")).unwrap();
    let mut corpus: Value =
        serde_json::from_slice(&fs::read(fixture.join("corpus.json")).unwrap()).unwrap();
    match edit {
        Edit::Original => {}
        Edit::Label => turtle.push_str("\ne:echo w:label \"Internal description\" .\n"),
        Edit::Rename => {
            turtle = turtle.replace("w:name \"echo\"", "w:name \"mirror\"");
            for call in corpus[0]["calls"].as_array_mut().unwrap() {
                call["function"] = json!("mirror");
            }
        }
        Edit::SwapTags => {
            turtle = turtle
                .replace(
                    "w:name \"empty\" ; w:ordinal 0",
                    "w:name \"empty\" ; w:ordinal 1",
                )
                .replace(
                    "w:name \"number\" ; w:ordinal 1",
                    "w:name \"number\" ; w:ordinal 0",
                );
        }
        Edit::RequiredImport => {
            turtle.push_str("\ne:world w:import e:seed .\ne:seed a w:Function ; w:name \"seed\" ; w:result \"s64\" .\n");
            corpus[0]["covers"]
                .as_array_mut()
                .unwrap()
                .push(json!({"declaration":"urn:oddlang:echo:seed","role":"wasm.importer"}));
            corpus[0]["imports"] = json!([{"function":"seed","arguments":[],"returns":"1"}]);
        }
    }
    fs::write(source.join("model.ttl"), turtle).unwrap();
    fs::write(
        source.join("corpus.json"),
        serde_json::to_vec(&corpus).unwrap(),
    )
    .unwrap();
    command(&[
        "publish",
        source.to_str().unwrap(),
        "contracts/wasm/empty.lock.json",
        "contracts/cache",
        root.join("bundle").to_str().unwrap(),
    ]);
}

fn command(args: &[&str]) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .arg("wasm")
        .args(args)
        .output()
        .unwrap();
    success(output)
}

fn success(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn pinned_diff_reports_surface_in_both_directions() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    publish(&original, Edit::Original);
    let old = original.join("bundle");
    for (name, edit, surface, id) in [
        ("label", Edit::Label, "surface-equal", None),
        (
            "rename",
            Edit::Rename,
            "surface-changed",
            Some("urn:oddlang:echo:echo"),
        ),
        (
            "tags",
            Edit::SwapTags,
            "surface-changed",
            Some("urn:oddlang:echo:number"),
        ),
        (
            "import",
            Edit::RequiredImport,
            "surface-changed",
            Some("urn:oddlang:echo:seed"),
        ),
    ] {
        let candidate = temp.path().join(name);
        publish(&candidate, edit);
        let new = candidate.join("bundle");
        for direction in ["old-host-new-guest", "new-host-old-guest"] {
            for format in ["text", "json"] {
                let report = command(&[
                    "diff",
                    old.to_str().unwrap(),
                    old.join("workspace.lock.json").to_str().unwrap(),
                    new.to_str().unwrap(),
                    new.join("workspace.lock.json").to_str().unwrap(),
                    direction,
                    format,
                ]);
                if format == "json" {
                    let data = serde_json::from_str::<Value>(&report).unwrap();
                    assert_eq!(data["direction"], direction);
                    if name == "import" {
                        let seed = data["changes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|entry| entry["iri"] == "urn:oddlang:echo:seed")
                            .unwrap();
                        let expected = if direction == "old-host-new-guest" {
                            "host"
                        } else {
                            "none"
                        };
                        assert_eq!(seed["endpoint"], expected);
                    }
                }
                assert!(report.contains(surface), "{report}");
                assert!(!report.to_lowercase().contains("compatible"), "{report}");
                if let Some(id) = id {
                    assert!(report.contains(id), "{report}");
                }
            }
        }
    }
}
