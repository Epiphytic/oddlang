use serde_json::Value;
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
    let root = temp.path();
    let old = root.join("echo");
    command(&[
        "publish",
        "contracts/wasm/echo",
        "contracts/wasm/empty.lock.json",
        "contracts/cache",
        old.to_str().unwrap(),
    ]);
    success(
        std::process::Command::new(env!("CARGO_BIN_EXE_publish-wasm-matrix"))
            .arg(root)
            .output()
            .unwrap(),
    );
    for (name, surface, id) in [
        ("label", "surface-equal", None),
        ("rename", "surface-changed", Some("urn:oddlang:echo:echo")),
        ("tags", "surface-changed", Some("urn:oddlang:echo:number")),
        ("import", "surface-changed", Some("urn:oddlang:echo:seed")),
    ] {
        let new = root.join(format!("echo-{name}"));
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
