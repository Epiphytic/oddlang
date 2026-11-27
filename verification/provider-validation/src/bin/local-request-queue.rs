//! Qualify generated queue persistence using the independent river-gauge consumer.
use provider_validation::{Result, cli, command, invoke, pin, subprocess::run_with_timeout, write};
use serde_json::json;
use std::{env, fs, path::Path, process::Command, thread, time::Duration};

fn race(observer: &Path, database: &Path) -> Result {
    let run = || -> std::result::Result<Vec<u8>, String> {
        let output = run_with_timeout(
            Command::new(observer).arg("race").arg(database),
            None,
            Duration::from_secs(10),
        )
        .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "race process {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    };
    let mut values = thread::scope(|scope| -> Result<_> {
        let left = scope.spawn(run);
        let right = scope.spawn(run);
        Ok([
            left.join().map_err(|_| "race worker panicked")??,
            right.join().map_err(|_| "race worker panicked")??,
        ])
    })?;
    values.sort();
    if values != [b"already queued\n".as_slice(), b"queued\n".as_slice()] {
        return Err(format!("unexpected concurrent queue results: {values:?}").into());
    }
    Ok(())
}

fn main() -> Result {
    let cli = cli()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../local-request-queue");
    let tmp = tempfile::Builder::new()
        .prefix("oddlang-river-gauge-queue-")
        .tempdir()?;
    let root = tmp.path();
    fs::copy(fixture.join("Cargo.lock"), root.join("Cargo.lock"))?;
    let graph = json!({"format":"oddlang-local-request-queue-v1","id":"urn:gauge:reading-queue","accountBytes":4,"roomBytes":8,"idBytes":16,"maximumFrameBytes":4});
    write(&root.join("queue.json"), &graph)?;
    write(
        &root.join("rust.json"),
        &json!({"format":"oddlang-local-request-queue-rust-v1","storage":"sqlite"}),
    )?;
    let mut manifest = json!({
        "format":"oddlang-native-package-v1","name":"gauge-reading-queue","version":"0.0.0","edition":"2021",
        "cargoLock":pin(root,"Cargo.lock")?,
        "dependencies":{"rusqlite":{"version":"0.40.1","defaultFeatures":false,"features":["bundled-sqlcipher-vendored-openssl"]},"sha2":{"version":"0.10.9"}},
        "registryPatches":{"libsqlite3-sys":{"git":"https://github.com/rusqlite/rusqlite.git","revision":"5ae7fdf83085c595fd54f977b3a56ccacabaf16b"}},
        "components":[{"kind":"local-request-queue","module":"queue","visibility":"public","graph":pin(root,"queue.json")?,"overlay":pin(root,"rust.json")?,
        "reexports":["Account","Room","Id","QueueScope","RequestKind","Request","QueueReceipt","Enqueued","QueuedFrame","QueueError","LocalQueue"]}]
    });
    let package = root.join("package.json");
    write(&package, &manifest)?;
    let build = ["build", "native-package", "rust", "package.json"];
    invoke(&cli, root, &build, "out", None)?;
    let output = root.join("out");
    fs::create_dir(output.join("examples"))?;
    fs::copy(
        fixture.join("consumer.rs"),
        output.join("examples/observe.rs"),
    )?;
    let artifacts = command(
        Command::new("cargo").env("CARGO_ENCODED_RUSTFLAGS", "-Dwarnings"),
        &output,
        &[
            "+1.97.1",
            "build",
            "--example",
            "observe",
            "--offline",
            "--locked",
            "--message-format=json",
        ],
        None,
    )?;
    let mut observer = None;
    for line in artifacts
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row: serde_json::Value = serde_json::from_slice(line)?;
        if row["reason"] == "compiler-artifact" && row["target"]["name"] == "observe" {
            observer = row["executable"].as_str().map(std::path::PathBuf::from);
        }
    }
    let observer = observer.ok_or("Cargo did not report the built observer")?;
    let database = root.join("readings.sqlite3");
    for phase in ["write", "recover"] {
        command(
            &mut Command::new(&observer),
            root,
            &[phase, "readings.sqlite3"],
            None,
        )?;
    }
    race(&observer, &database)?;
    if fs::read(&database)?.starts_with(b"SQLite format 3\0") {
        return Err("found plaintext SQLite header".into());
    }
    for (field, value) in [
        ("maximumFrameBytes", json!(0)),
        ("maximumFrameBytes", json!(1_048_577)),
        ("accountBytes", json!(0)),
        ("idBytes", json!(65)),
        ("id", json!("invalid")),
        ("id", json!("urn:")),
        ("id", json!("urn:spaces invalid")),
        ("id", json!(format!("urn:{}", "a".repeat(256)))),
    ] {
        let mut invalid = graph.clone();
        invalid[field] = value;
        write(&root.join("queue.json"), &invalid)?;
        manifest["components"][0]["graph"] = pin(root, "queue.json")?;
        write(&package, &manifest)?;
        invoke(
            &cli,
            root,
            &build,
            "invalid",
            Some("invalid local request queue graph"),
        )?;
    }
    println!("durable local queue fixture passed");
    Ok(())
}
