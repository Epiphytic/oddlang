use std::{
    fs,
    path::{Path, PathBuf},
};
pub const BALANCE: &str = "urn:oddlang:ledger:balance";
pub fn v1() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/ledger/v1")
}
pub fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for f in fs::read_dir(v1()).unwrap().flatten() {
        if f.path().extension().is_some_and(|e| e == "ttl") {
            fs::copy(f.path(), t.path().join(f.file_name())).unwrap();
        }
    }
    t
}
pub fn replace(p: &Path, file: &str, from: &str, to: &str) {
    let path = p.join(file);
    let s = fs::read_to_string(&path).unwrap();
    assert!(s.contains(from), "{file}: {from}");
    fs::write(path, s.replace(from, to)).unwrap();
}
pub fn rewrite(p: &Path) {
    for f in fs::read_dir(p).unwrap().flatten() {
        if f.path().extension().is_none_or(|e| e != "ttl") {
            continue;
        }
        let s = fs::read_to_string(f.path()).unwrap();
        let (prefix, mut triples): (Vec<_>, Vec<_>) = s
            .lines()
            .filter(|s| !s.trim().is_empty())
            .partition(|s| s.starts_with("@prefix"));
        triples.reverse();
        fs::write(
            f.path(),
            prefix
                .into_iter()
                .chain(triples)
                .collect::<Vec<_>>()
                .join("\n")
                .replace("l:", "ledger:")
                .replace("_:bounds", "_:other"),
        )
        .unwrap();
    }
}
