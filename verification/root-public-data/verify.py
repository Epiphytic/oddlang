#!/usr/bin/env python3
"""Check source recovery of an unrelated root record and unit error."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

cli = Path(sys.argv[1]).resolve()
source = '''
#![cfg(unix)]
use thiserror::Error;
#[derive(Debug, Error)]
pub enum Fault {
    #[error("sample missing")]
    Empty,
    #[error("sensor unavailable")]
    Unavailable,
}
#[derive(Debug)]
pub struct Snapshot {
    pub secondary: String,
    pub primary: String,
}
pub fn is_empty(_: &Snapshot) -> bool { false }
'''
caller = r'''
#[path = "records.rs"] mod public;
#[path = "errors.rs"] mod failure;
fn main() {
    let snapshot = public::Snapshot { primary: "first".into(), secondary: "second".into() };
    assert_eq!(format!("{:?}", snapshot), r#"Snapshot { secondary: "second", primary: "first" }"#);
    let fault = failure::Fault::Empty;
    assert_eq!(format!("{:?}", fault), "Empty");
    assert_eq!(fault.to_string(), "sample missing");
    assert!(std::error::Error::source(&fault).is_none());
    assert_eq!(failure::Fault::Unavailable.to_string(), "sensor unavailable");
}
'''


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


def reject(*args):
    result = subprocess.run(args, text=True, capture_output=True)
    assert result.returncode, result.stdout


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    rust, sel = root / "lib.rs", root / "selection.json"

    def recovery(text, command, output, check=True):
        rust.write_text(text)
        sha = hashlib.sha256(text.encode()).hexdigest()
        base = dict(sourceSha256=sha, id="urn:sensor:public")
        if command == "finite-error-domain":
            base.update(format="oddlang-finite-error-selection-v1", error="Fault")
        else:
            base.update(format="oddlang-rust-root-record-selection-v1",
                        records={"Snapshot": "urn:sensor:snapshot"})
        sel.write_text(json.dumps(base))
        return (run if check else reject)(cli, "recover", command, rust, sel, output)

    for command, target, file in (("finite-error-domain", "finite-error-domain", "errors.rs"),
                                  ("public-root-records", "public-records", "records.rs")):
        extracted, projected = root / command, root / (command + "-projected")
        recovery(source, command, extracted)
        run(cli, "build", target, "rust", extracted / "graph.json", extracted / "rust.json", projected)
        (root / file).write_bytes((projected / file).read_bytes())
    (root / "main.rs").write_text(caller)
    run("rustc", "--edition=2021", root / "main.rs", "-o", root / "probe")
    run(root / "probe")
    for index, text in enumerate((
        source.replace('Empty,', 'Empty(u8),'),
        source.replace('Empty,', 'Empty = 3,'),
        source.replace('use thiserror::Error;', 'use fake::Error;'),
        source + '\npub type Result<T> = std::result::Result<T, Fault>;',
        source + '\nimpl From<Fault> for u8 { fn from(_: Fault) -> u8 { 0 } }',
        source + '\nmacro_rules! more { () => {} } more!();',
    )):
        recovery(text, "finite-error-domain", root / f"bad-error-{index}", False)
    for index, text in enumerate((
        source.replace('pub secondary: String', 'secondary: String'),
        source.replace('pub primary: String', 'pub primary: u8'),
        source + '\nuse custom::String;',
        source + '\nmacro_rules! more { () => {} } more!();',
        source.replace('#![cfg(unix)]', '#![cfg(target_arch = "aarch64")]'),
    )):
        recovery(text, "public-root-records", root / f"bad-record-{index}", False)

    record = "#[derive(Debug)] pub struct Snapshot { pub primary: String }"
    for index, text in enumerate((
        "mod m { pub use ::core::clone::Clone as Debug; } use self::m::r#Debug; " + record,
        "mod m { pub type String = u8; } use self::m::r#String; " + record,
        "type r#String = u8; " + record,
        "mod m { pub mod r#core {} } use self::m::r#core::{self}; " + record,
    )):
        recovery(text, "public-root-records", root / f"raw-shadow-{index}", False)
