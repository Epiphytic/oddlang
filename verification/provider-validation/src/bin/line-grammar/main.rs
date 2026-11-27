use provider_validation::{Result, read, sha, subprocess, write};
use serde::Deserialize;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

mod inventory;
mod records;
mod support;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../line-grammar");
struct Suite {
    cli: PathBuf,
    work: tempfile::TempDir,
    fixture: Value,
}

fn run(cmd: &mut Command, expected: Option<&str>) -> Result {
    let result = subprocess::run(cmd, None)?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    match expected {
        None if result.status.success() => Ok(()),
        Some(message) if !result.status.success() && stderr.contains(message) => Ok(()),
        _ => Err(format!(
            "{cmd:?}: expected {expected:?}, got {}\n{}{stderr}",
            result.status,
            String::from_utf8_lossy(&result.stdout)
        )
        .into()),
    }
}

impl Suite {
    fn root(&self) -> &Path {
        self.work.path()
    }
    fn build(&self, kind: &str, inputs: &[&str], out: &str, expected: Option<&str>) -> Result {
        run(
            Command::new(&self.cli)
                .current_dir(self.root())
                .args(["build", kind, "rust"])
                .args(inputs)
                .arg(out),
            expected,
        )?;
        if self.root().join(out).exists() == expected.is_some() {
            return Err(format!("unexpected publication state of {out}").into());
        }
        Ok(())
    }
    fn project(&self, fixture: &Value, out: &str, expected: Option<&str>) -> Result {
        for key in ["graph", "overlay"] {
            write(&self.root().join(format!("{key}.json")), &fixture[key])?;
        }
        self.build(
            "line-grammar",
            &["graph.json", "overlay.json"],
            out,
            expected,
        )
    }
    fn witness(&self, source: &str, deny_warnings: bool, vars: &[(&str, &str)]) -> Result {
        let mut compiler = Command::new("rustc");
        compiler
            .current_dir(self.root())
            .args(["+1.97.1", "--edition=2021"]);
        if deny_warnings {
            compiler.arg("-Dwarnings");
        }
        for (var, value) in vars {
            compiler.env(var, self.root().join(value));
        }
        run(
            compiler
                .arg(Path::new(FIXTURES).join(source))
                .args(["-o", "caller"]),
            None,
        )?;
        run(
            Command::new(self.root().join("caller")).current_dir(self.root()),
            None,
        )
    }
}

#[derive(Deserialize)]
struct Rejection {
    case: String,
    diagnostic: String,
    changes: Vec<Change>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Change {
    Add { path: String, value: Value },
    Replace { path: String, value: Value },
    Remove { path: String },
}
impl Change {
    fn apply(self, fixture: &mut Value) -> Result {
        let (path, value) = match self {
            Self::Replace { path, value } => {
                *fixture
                    .pointer_mut(&path)
                    .ok_or_else(|| format!("unknown fixture pointer {path}"))? = value;
                return Ok(());
            }
            Self::Add { path, value } => (path, Some(value)),
            Self::Remove { path } => (path, None),
        };
        let (prefix, key) = path.rsplit_once('/').ok_or("invalid fixture pointer")?;
        match (fixture.pointer_mut(prefix), value) {
            (Some(Value::Object(obj)), Some(value)) if !obj.contains_key(key) => {
                obj.insert(key.to_owned(), value);
            }
            (Some(Value::Object(obj)), None) if obj.contains_key(key) => {
                obj.remove(key);
            }
            (Some(Value::Array(array)), Some(value)) if key == "-" => array.push(value),
            _ => return Err(format!("invalid fixture change at {path}").into()),
        }
        Ok(())
    }
}

fn main() -> Result {
    let args: Vec<_> = env::args().skip(1).collect();
    let [suite, cli] = args.as_slice() else {
        return Err("expected inventory|records|support ODDLANG".into());
    };
    let verify: fn(&Suite) -> Result = match suite.as_str() {
        "inventory" => inventory::verify,
        "records" => records::verify,
        "support" => support::verify,
        _ => return Err("unknown line grammar suite".into()),
    };
    let fixtures = Path::new(FIXTURES);
    let context = Suite {
        work: tempfile::tempdir()?,
        cli: Path::new(cli).canonicalize()?,
        fixture: read(&fixtures.join(format!("{suite}.json")))?,
    };
    context.project(&context.fixture, "generated", None)?;
    verify(&context)?;
    let cases: Vec<Rejection> = serde_json::from_slice(&fs::read(
        fixtures.join(format!("{suite}-rejections.json")),
    )?)?;
    let count = cases.len();
    for case in cases {
        let mut invalid = context.fixture.clone();
        for change in case.changes {
            change.apply(&mut invalid)?;
        }
        context
            .project(&invalid, "rejected", Some(&case.diagnostic))
            .map_err(|e| format!("{}: {e}", case.case))?;
    }
    println!("Line grammar {suite} native witnesses and {count} rejection cases passed");
    Ok(())
}
