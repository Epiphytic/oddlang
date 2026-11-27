use super::*;
use provider_validation::write;

fn witnesses() -> Value {
    let rows = json!({"debit":{"id":"debit","owner":"","balance":1},"credit":{"id":"credit","owner":"","balance":i64::MAX}});
    let mut changed = rows.clone();
    changed["debit"]["balance"] = json!(0);
    let overflow = format!("{ERROR}DestinationOverflow");
    json!({"guardOrder":{"input":{"source":"","destination":"","amount":-1,"rows":{}},
        "expected":{"status":format!("{ERROR}SourceMissing"),"rows":{}},"actual":{"status":format!("{ERROR}NonPositiveAmount"),"rows":{}}},
        "earlyDebit":{"input":{"source":"debit","destination":"credit","amount":1,"rows":rows},
            "expected":{"status":overflow,"rows":rows},"actual":{"status":overflow,"rows":changed}}})
}
#[test]
fn both_witnesses() {
    mutants(&witnesses()).unwrap();
}
#[test]
fn equal_outcomes_do_not_disprove_guard_order() {
    let mut m = witnesses();
    m["guardOrder"]["actual"] = m["guardOrder"]["expected"].clone();
    assert!(
        mutants(&m)
            .unwrap_err()
            .to_string()
            .contains("guard mutant")
    );
}
#[test]
fn changed_owner_is_not_a_debit_witness() {
    let mut m = witnesses();
    m["earlyDebit"]["actual"]["rows"]["debit"]["owner"] = json!("changed");
    assert!(
        mutants(&m)
            .unwrap_err()
            .to_string()
            .contains("premature state")
    );
}
fn copy_dir(src: &Path, dst: &Path) -> Result {
    fs::create_dir_all(dst)?;
    for entry in src.read_dir()? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
struct Archive {
    root: PathBuf,
    report: tempfile::TempDir,
}
impl Archive {
    fn open() -> Result<Self> {
        let source = env::var_os("ODDLANG_REPLAY_FIXTURE")
            .ok_or("set ODDLANG_REPLAY_FIXTURE to a collected report")?;
        let root = env::var_os("ODDLANG_REPLAY_SOURCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .nth(2)
                    .unwrap()
                    .into()
            });
        let report = tempfile::tempdir()?;
        copy_dir(&PathBuf::from(source), report.path())?;
        check(&root, report.path(), false)?;
        Ok(Self { root, report })
    }
    fn rejected(&self, error: &str) {
        let got = check(&self.root, self.report.path(), false).unwrap_err();
        assert!(got.to_string().contains(error), "{got}");
    }
    fn missing(&self, file: &str) -> Result {
        fs::remove_file(self.report.path().join(file))?;
        let got = check(&self.root, self.report.path(), false).unwrap_err();
        assert_eq!(
            got.downcast_ref::<std::io::Error>().map(|e| e.kind()),
            Some(std::io::ErrorKind::NotFound)
        );
        Ok(())
    }
    fn append(&self, file: &str) -> Result {
        Ok(fs::OpenOptions::new()
            .append(true)
            .open(self.report.path().join(file))?
            .write_all(b"\n")?)
    }
    fn edit(&self, file: &str, edit: impl FnOnce(&mut Value)) -> Result {
        let path = self.report.path().join(file);
        let mut doc = read(&path)?;
        edit(&mut doc);
        write(&path, &doc)
    }
}
#[test]
#[ignore = "requires a collected translation report"]
fn missing_report() -> Result {
    Archive::open()?.missing("proof/result.json")
}
#[test]
#[ignore = "requires a collected translation report"]
fn missing_summary() -> Result {
    Archive::open()?.missing("summary.json")
}
#[test]
#[ignore = "requires a collected translation report"]
fn stale_build() -> Result {
    let a = Archive::open()?;
    a.append("generated/account.rs")?;
    a.rejected("stale differential artifact: generated");
    Ok(())
}
#[test]
#[ignore = "requires a collected translation report"]
fn stale_proof_log() -> Result {
    let a = Archive::open()?;
    a.append("proof/transfer_equivalence.log")?;
    a.rejected("stale proof output");
    Ok(())
}
#[test]
#[ignore = "requires a collected translation report"]
fn unbounded_proof_claim() -> Result {
    let a = Archive::open()?;
    a.edit("proof/result.json", |v| {
        v["unboundedKeysAndMaps"] = json!(PROVED)
    })?;
    a.rejected("unboundedKeysAndMaps");
    Ok(())
}
#[test]
#[ignore = "requires a collected translation report"]
fn false_domain_promotion() -> Result {
    let a = Archive::open()?;
    a.edit("summary.json", |v| {
        v["claims"]["arbitraryLedger"]["classification"] = json!("proved")
    })?;
    a.rejected("differs from recomputed");
    Ok(())
}
#[test]
#[ignore = "requires a collected translation report"]
fn missing_outcome() -> Result {
    let a = Archive::open()?;
    a.edit("differential.json", |v| {
        v["outcomes"].as_object_mut().unwrap().remove("ok");
    })?;
    a.rejected("reachable outcome");
    Ok(())
}
