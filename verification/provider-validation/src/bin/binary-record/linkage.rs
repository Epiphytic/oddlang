use super::*;
use provider_validation::{invoke, pin, write};
use schema_support::{Change, apply};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    error: Option<String>,
    base: Option<String>,
    #[serde(default)]
    changes: Vec<Change>,
    #[serde(default)]
    add_components: Vec<Value>,
    #[serde(default)]
    drop_last_component: bool,
    #[serde(default)]
    duplicate_layer: bool,
}

fn assemble(s: &Suite, mut model: Value, name: &str, error: Option<&str>) -> Result {
    let stale_lock = model.as_object_mut().unwrap().remove("staleLock").is_some();
    model["cargoLock"] = pin(&s.root, "Cargo.lock")?;
    if stale_lock {
        model["cargoLock"]["sha256"] = "0".repeat(64).into();
    }
    for (index, component) in model["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let stale = component.as_object_mut().unwrap().remove("stalePin");
        for key in ["graph", "overlay"] {
            let file = format!("component-{index}-{key}.json");
            let path = s.root.join(&file);
            write(&path, &component[key])?;
            component[key] = pin(&s.root, &file)?;
            if stale.as_ref().and_then(Value::as_str) == Some(key) {
                use std::io::Write;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&path)?
                    .write_all(b"\n")?;
            }
        }
    }
    write(&s.root.join("manifest.json"), &model)?;
    invoke(
        &s.cli,
        &s.root,
        &["build", "native-package", "rust", "manifest.json"],
        name,
        error,
    )
}

pub(super) fn qualify(s: &Suite, base: Value, cases: &str) -> Result {
    cargo(&s.root, &["generate-lockfile", "--offline"])?;
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(s.case(cases))?)?;
    let mut snapshots = BTreeMap::<String, Value>::new();
    for case in cases {
        let mut package = match case.base {
            Some(name) => snapshots
                .get(&name)
                .ok_or_else(|| format!("unknown base {name}"))?
                .clone(),
            None => base.clone(),
        };
        let components = package["components"]
            .as_array_mut()
            .ok_or("missing components")?;
        components.extend(case.add_components);
        if case.drop_last_component {
            components.pop().ok_or("missing layer component")?;
        }
        if case.duplicate_layer {
            let mut extra = components[2].clone();
            extra["module"] = "other".into();
            components.push(extra);
        }
        apply(&mut package, case.changes)?;
        assemble(s, package.clone(), &case.name, case.error.as_deref())?;
        snapshots.insert(case.name, package);
    }
    Ok(())
}
fn sources(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) -> Result {
    for child in fs::read_dir(path)? {
        let child = child?.path();
        if child.is_dir() {
            sources(root, &child, files)?;
        } else if child.extension().is_some_and(|ext| ext == "rs") {
            files.insert(child.strip_prefix(root)?.to_owned(), fs::read(child)?);
        }
    }
    Ok(())
}
pub(super) fn geology(s: &Suite) -> Result {
    fs::write(
        s.root.join("Cargo.toml"),
        "[package]\nname=\"geology\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[lib]\npath=\"placeholder.rs\"\n[workspace]\n",
    )?;
    qualify(s, s.json("geology-package.json")?, "geology-linkage.json")?;
    s.compare("accepted", "repeat", &["package.lock"])?;
    let trees = ["accepted", "repeat"].map(|name| -> Result<_> {
        let root = s.root.join(name);
        let mut files = BTreeMap::new();
        sources(&root, &root, &mut files)?;
        Ok(files)
    });
    let [first, second] = trees;
    assert_eq!(first?, second?);
    for (name, witness) in [("accepted", "native.rs"), ("independent-caps", "widths.rs")] {
        let root = s.root.join(name);
        fs::create_dir(root.join("tests"))?;
        fs::copy(s.case(witness), root.join("tests").join(witness))?;
        cargo(&root, &["test", "--locked", "--offline"])?;
    }
    Ok(())
}
