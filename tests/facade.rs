use oddlang::facade::{build_pinned, import, verify_pinned};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn fixture(p: &Path) {
    fs::create_dir_all(p.join("src")).unwrap();
    fs::write(
        p.join("Cargo.toml"),
        "[package]\nname='example-facade'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        p.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = 'example-facade'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(p.join("facade.toml"),"profile='rust-facade-v1'\npackage='example-facade'\n[exports]\nleft=['Ticket']\nright=['State']\n").unwrap();
    fs::write(
        p.join("src/lib.rs"),
        "mod right;\nmod left;\npub use right::{State};\npub use left::Ticket;\n",
    )
    .unwrap();
    fs::write(p.join("src/left.rs"), "pub struct Ticket(i32);\n").unwrap();
    fs::write(p.join("src/right.rs"), "pub enum State { Open, Closed }\n").unwrap();
}
#[test]
fn project_pinned_facade_after_deleting_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixture(&source);
    let out = temp.path().join("import");
    let dest = temp.path().join("projection");
    import(
        &source.join("Cargo.toml"),
        &source.join("facade.toml"),
        &out,
    )
    .unwrap();
    let pin = sha(&fs::read(out.join("import.lock")).unwrap());
    fs::remove_dir_all(source).unwrap();
    verify_pinned(&out, &pin).unwrap();
    build_pinned(&out, &pin, &dest).unwrap();
    let emitted = fs::read_to_string(dest.join("lib.rs")).unwrap();
    assert!(emitted.contains("pub use left::Ticket;"));
    assert!(emitted.contains("pub use right::State;"));
    assert!(!emitted.contains("struct Ticket"));
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 2);
}

#[test]
fn reject_unresolved_ambiguous_and_conditional_exports() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("lib");
    fixture(&root);
    let original = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let mapping = root.join("facade.toml");
    for (from, to) in [
        ("pub use left::Ticket;", "pub use left::Missing;"),
        ("pub use left::Ticket;", "pub use left::Ticket as State;"),
        ("pub use right::{State};", "pub use right::{State,State};"),
        ("mod right;", "#[cfg(unix)] mod right;"),
        ("mod right;", "#[path = \"alternate.rs\"] mod right;"),
        ("pub use left::Ticket;", "pub use left::*;"),
        (
            "pub use left::Ticket;",
            "pub use left::Ticket; fn main() {}",
        ),
    ] {
        fs::write(root.join("src/lib.rs"), original.replace(from, to)).unwrap();
        let attempt =
            || import(&root.join("Cargo.toml"), &mapping, &tmp.path().join("out")).unwrap_err();
        assert_eq!(attempt(), attempt());
        assert!(!tmp.path().join("out").exists());
    }
    fs::write(root.join("src/lib.rs"), original).unwrap();
    fs::write(
        root.join("src/right.rs"),
        "#[cfg(unix)] pub enum State { Open }",
    )
    .unwrap();
    assert!(
        import(&root.join("Cargo.toml"), &mapping, &tmp.path().join("out"))
            .unwrap_err()
            .contains("attribute")
    );
    fs::remove_file(root.join("src/right.rs")).unwrap();
    assert!(
        import(&root.join("Cargo.toml"), &mapping, &tmp.path().join("out"))
            .unwrap_err()
            .contains("right.rs")
    );
}

#[test]
fn independent_pin_and_source_witnesses_reject_substitution() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    fixture(&src);
    let out = tmp.path().join("original");
    let new = tmp.path().join("different");
    let run = |out: &Path| import(&src.join("Cargo.toml"), &src.join("facade.toml"), out).unwrap();
    run(&out);
    let pin = sha(&fs::read(out.join("import.lock")).unwrap());
    let source = fs::read(src.join("src/lib.rs")).unwrap();
    let m: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("source-map.json")).unwrap()).unwrap();
    assert_eq!(m["fileSha256"], sha(&source));
    for statement in fs::read_to_string(out.join("domain.ttl")).unwrap().lines() {
        let site = &m["statementWitnesses"][sha(statement.as_bytes())];
        let range = site["byteStart"].as_u64().unwrap() as usize
            ..site["byteEnd"].as_u64().unwrap() as usize;
        assert_eq!(site["spanSha256"], sha(&source[range]));
    }
    fs::write(
        src.join("src/lib.rs"),
        String::from_utf8(source).unwrap().replace("Ticket", "Pass"),
    )
    .unwrap();
    fs::write(src.join("src/left.rs"), "pub struct Pass(i32);").unwrap();
    let map = fs::read_to_string(src.join("facade.toml"))
        .unwrap()
        .replace("Ticket", "Pass");
    fs::write(src.join("facade.toml"), map).unwrap();
    run(&new);
    let new_pin = sha(&fs::read(new.join("import.lock")).unwrap());
    verify_pinned(&new, &new_pin).unwrap();
    assert!(
        verify_pinned(&new, &pin)
            .unwrap_err()
            .contains("pin mismatch")
    );
}

#[test]
fn generated_library_serves_the_same_public_types_to_a_separate_crate() {
    use std::process::Command;
    let tmp = tempfile::tempdir().unwrap();
    let original = tmp.path().join("original");
    fixture(&original);
    let dir = tmp.path().join("import");
    let projected = tmp.path().join("projection");
    import(
        &original.join("Cargo.toml"),
        &original.join("facade.toml"),
        &dir,
    )
    .unwrap();
    let pin = sha(&fs::read(dir.join("import.lock")).unwrap());
    let library = tmp.path().join("library");
    fixture(&library);
    fs::remove_file(library.join("src/lib.rs")).unwrap();
    fs::remove_dir_all(original).unwrap();
    build_pinned(&dir, &pin, &projected).unwrap();
    fs::copy(projected.join("lib.rs"), library.join("src/lib.rs")).unwrap();
    let consumer = tmp.path().join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"),"[workspace]\n[package]\nname='external'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nfixture={package='example-facade',path='../library'}\n").unwrap();
    fs::write(
        consumer.join("src/lib.rs"),
        "pub fn select(_: fixture::Ticket) -> fixture::State { fixture::State::Open }",
    )
    .unwrap();
    let status = Command::new("cargo")
        .args(["+1.97.1", "check", "--offline", "--quiet"])
        .current_dir(&consumer)
        .status()
        .unwrap();
    assert!(status.success());
}
