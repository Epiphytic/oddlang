use provider_validation::{Result, invoke, read, write};
use serde::Deserialize;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub struct Suite {
    pub cli: PathBuf,
    pub root: PathBuf,
    pub fixtures: PathBuf,
    kind: &'static str,
    _scratch: tempfile::TempDir,
}

impl Suite {
    pub fn open(kind: &'static str) -> Result<(String, Self)> {
        let args: Vec<_> = env::args_os().skip(1).collect();
        let [name, cli] = args.as_slice() else {
            return Err("expected SUITE ODDLANG".into());
        };
        let scratch = tempfile::tempdir()?;
        Ok((
            name.to_str().ok_or("invalid suite name")?.to_owned(),
            Self {
                cli: Path::new(cli).canonicalize()?,
                root: scratch.path().to_owned(),
                fixtures: Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(kind),
                kind,
                _scratch: scratch,
            },
        ))
    }
    pub fn case(&self, name: &str) -> PathBuf {
        self.fixtures.join("cases").join(name)
    }
    pub fn json(&self, name: &str) -> Result<Value> {
        read(&self.case(name))
    }
    pub fn project(&self, model: &Value, out: &str, error: Option<&str>) -> Result {
        write(&self.root.join("graph.json"), &model["graph"])?;
        write(&self.root.join("rust.json"), &model["binding"])?;
        invoke(
            &self.cli,
            &self.root,
            &["build", self.kind, "rust", "graph.json", "rust.json"],
            out,
            error,
        )
    }
    pub fn rejections(&self, model: &Value, filename: &str) -> Result {
        #[derive(Deserialize)]
        struct Rejection {
            name: String,
            error: String,
            changes: Vec<Change>,
        }
        let cases: Vec<Rejection> = serde_json::from_slice(&fs::read(self.case(filename))?)?;
        for case in cases {
            let mut bad = model.clone();
            apply(&mut bad, case.changes)?;
            self.project(&bad, &case.name, Some(&case.error))?;
        }
        Ok(())
    }
    pub fn compare(&self, first: &str, second: &str, files: &[&str]) -> Result {
        for name in files {
            assert_eq!(
                fs::read(self.root.join(first).join(name))?,
                fs::read(self.root.join(second).join(name))?,
                "{name}"
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Change {
    Remove { remove: String },
    Set { set: String, value: Value },
}
pub fn apply(doc: &mut Value, changes: Vec<Change>) -> Result {
    for change in changes {
        let (path, value) = match change {
            Change::Remove { remove } => (remove, None),
            Change::Set { set, value } => (set, Some(value)),
        };
        let (prefix, key) = path.rsplit_once('/').ok_or("expected a fixture pointer")?;
        match (doc.pointer_mut(prefix), value) {
            (Some(Value::Object(obj)), Some(v)) => {
                obj.insert(key.to_string(), v);
            }
            (Some(Value::Object(obj)), None) if obj.contains_key(key) => {
                obj.remove(key);
            }
            (Some(Value::Array(array)), Some(v)) => {
                *array
                    .get_mut(key.parse::<usize>()?)
                    .ok_or("invalid fixture array index")? = v;
            }
            _ => return Err(format!("unknown fixture path: {path}").into()),
        }
    }
    Ok(())
}

// Each generated package owns its target directory, including packages with
// identical names and versions built from different qualification inputs.
pub fn cargo(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    provider_validation::command(
        std::process::Command::new("cargo").env("CARGO_TARGET_DIR", root.join("target")),
        root,
        args,
        None,
    )
}
