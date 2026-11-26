use std::{fs, path::Path, process::Command};

fn success(cmd: &mut Command) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn project_after_removing_original_crate_and_detect_changed_wit() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/framing-independent");
    let stage = tempfile::tempdir().unwrap();
    let source = stage.path().join("source");
    let consumer = stage.path().join("consumer");
    for dir in [&source, &consumer] {
        fs::create_dir_all(dir.join("src")).unwrap();
        for name in ["Cargo.toml", "Cargo.lock"] {
            fs::copy(fixture.join(name), dir.join(name)).unwrap();
        }
    }
    for name in ["src/lib.rs", "src/frame.rs", "mapping.toml"] {
        fs::copy(fixture.join(name), source.join(name)).unwrap();
    }
    let import = stage.path().join("semantic");
    let projection = stage.path().join("native");
    let cli = || Command::new(env!("CARGO_BIN_EXE_oddlang"));
    success(
        cli()
            .args(["import", "rust"])
            .arg(source.join("Cargo.toml"))
            .arg(source.join("mapping.toml"))
            .arg(&import),
    );
    fs::remove_dir_all(source).unwrap();
    success(
        cli()
            .args(["build", "module"])
            .arg(&import)
            .arg(&projection),
    );
    success(
        cli()
            .args(["import", "verify-build"])
            .arg(&import)
            .arg(&projection),
    );
    let wit = projection.join("world.wit");
    wit_parser::Resolve::default().push_file(&wit).unwrap();
    fs::copy(projection.join("frame.rs"), consumer.join("src/frame.rs")).unwrap();
    fs::write(consumer.join("src/main.rs"), r##"mod frame;
use frame::{FrameError, ProbeEnvelope};

fn main() {
    let encoded = ProbeEnvelope::encode(&[1,2,3]).unwrap();
    assert_eq!(encoded, br#"{"version":7,"payload":"AQID"}"#);
    assert_eq!(ProbeEnvelope::decode(&encoded).unwrap(), [1,2,3]);
    assert_eq!(ProbeEnvelope::encode(&[0;8]), Err(FrameError::SizeLimit));
    assert_eq!(ProbeEnvelope::decode(br#"{"version":0,"payload":"AAAAAAAAAAAAAAAA"}"#), Err(FrameError::SizeLimit));
}
"##).unwrap();
    success(
        Command::new("cargo")
            .args(["+1.97.1", "run", "--locked", "--offline", "--quiet"])
            .current_dir(consumer),
    );
    fs::write(wit, "package example:wrong; world framing {}").unwrap();
    assert!(
        !cli()
            .args(["import", "verify-build"])
            .arg(import)
            .arg(projection)
            .output()
            .unwrap()
            .status
            .success()
    );
}
