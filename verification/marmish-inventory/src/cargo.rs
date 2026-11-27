use crate::*;
use std::process::{Command, Stdio};
pub fn run(root: &Path, command: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(root)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {command}"))?;
    if let Some(data) = input {
        child.stdin.take().unwrap().write_all(data)?;
    }
    let result = child.wait_with_output()?;
    ensure!(
        result.status.success(),
        "{command} {} failed ({})",
        args.join(" "),
        result.status
    );
    Ok(result.stdout)
}
pub fn inventory(root: &Path, project: &Value) -> Result<(Value, BTreeMap<String, String>)> {
    let args = ["metadata", "--locked", "--offline", "--format-version", "1"];
    let meta: Value = serde_json::from_slice(&run(root, "cargo", &args, None)?)?;
    let mut members = BTreeMap::new();
    let (mut crates, mut targets, mut external) = (Map::new(), Map::new(), Map::new());
    for p in array(&meta["packages"])? {
        let id = string(&p["id"])?;
        let name = string(&p["name"])?;
        if !array(&meta["workspace_members"])?.contains(&p["id"]) {
            external.insert(
                id.into(),
                json!({
                    "name": name,
                    "version": p["version"],
                    "source": p["source"],
                    "class": "external-library"
                }),
            );
            continue;
        }
        members.insert(id.to_owned(), name.to_owned());
        let mut deps = Vec::new();
        for d in array(&p["dependencies"])? {
            let dep: Map = [
                "name",
                "rename",
                "kind",
                "req",
                "target",
                "features",
                "uses_default_features",
                "optional",
            ]
            .into_iter()
            .map(|k| (k.to_owned(), d[k].clone()))
            .collect();
            deps.push(dep);
        }
        crates.insert(
            name.into(),
            json!({
                "version": p["version"],
                "edition": p["edition"],
                "features": p["features"],
                "dependencyDeclarations": deps
            }),
        );
        for t in array(&p["targets"])? {
            let kind = string(&t["kind"][0])?;
            let n = string(&t["name"])?;
            let path = Path::new(string(&t["src_path"])?).strip_prefix(root)?;
            targets.insert(
                format!("{name}:{kind}:{n}"),
                json!({
                    "crate": name,
                    "kind": kind,
                    "name": n,
                    "path": path,
                    "requiredFeatures": t.get("required-features").cloned().unwrap_or(json!([])),
                    "edition": t["edition"]
                }),
            );
        }
    }
    ensure!(
        members.values().cloned().collect::<Set>()
            == strings(&project["workspace_members"])?
                .into_iter()
                .collect(),
        "Cargo workspace differs from pinned manifest"
    );
    let pkg = |id: &str| {
        members
            .get(id)
            .map(|name| format!("workspace:{name}"))
            .unwrap_or_else(|| id.to_string())
    };
    let (mut pool, mut profiles) = (Map::new(), Map::new());
    for platform in strings(&project["targets"])? {
        let mut selected = Map::new();
        for feature in strings(&project["feature_profiles"])? {
            let mut args = args.to_vec();
            args.extend(["--filter-platform", &platform]);
            if feature != "default" {
                args.extend(["--features", &feature]);
            }
            let cfg: Value = serde_json::from_slice(&run(root, "cargo", &args, None)?)?;
            let mut hashes = Vec::new();
            for node in array(&cfg["resolve"]["nodes"])? {
                let mut features = strings(&node["features"])?;
                features.sort();
                let mut deps = vec![];
                for d in array(&node["deps"])? {
                    deps.push(json!([d["name"], pkg(string(&d["pkg"])?), d["dep_kinds"]]));
                }
                deps.sort_by_cached_key(encode);
                let row = json!({"id":pkg(string(&node["id"])?),"features":features,"deps":deps});
                let key = digest(&encode(&row));
                pool.insert(key.clone(), row);
                hashes.push(key);
            }
            hashes.sort();
            selected.insert(feature, json!(hashes));
        }
        profiles.insert(platform, json!(selected));
    }
    Ok((
        json!({
            "crates": crates,
            "targets": targets,
            "external": external,
            "resolvedNodes": pool,
            "profiles": profiles
        }),
        members,
    ))
}
pub fn framing(root: &Path, members: &BTreeMap<String, String>, project: &Value) -> Result<Value> {
    let compiler = String::from_utf8(run(root, "rustc", &["-Vv"], None)?)?
        .trim()
        .to_string();
    ensure!(
        compiler.lines().any(
            |line| line == format!("release: {}", project["rust"].as_str().unwrap_or_default())
        ),
        "Rust compiler differs from project"
    );
    let probe = include_str!("frame_consumer.txt");
    let tmp = tempfile::tempdir()?;
    let data = run(
        root,
        "cargo",
        &[
            "check",
            "--locked",
            "--offline",
            "--lib",
            "-p",
            "marmish-core",
            "--target-dir",
            tmp.path().to_str().unwrap(),
            "--message-format=json",
        ],
        None,
    )?;
    let mut compiled = Set::new();
    let mut rmeta = None;
    for line in data.split(|&b| b == b'\n').filter(|line| !line.is_empty()) {
        let a: Value = serde_json::from_slice(line)?;
        if a["reason"] != "compiler-artifact" {
            continue;
        }
        let id = string(&a["package_id"])?;
        if members.get(id).map(String::as_str) == Some("marmish-core")
            && a["target"]["kind"] == json!(["lib"])
        {
            rmeta = strings(&a["filenames"])?
                .into_iter()
                .find(|p| p.ends_with(".rmeta"));
        }
        compiled.insert(
            members
                .get(id)
                .map(|s| format!("workspace:{s}"))
                .unwrap_or_else(|| id.to_string()),
        );
    }
    let path = rmeta.context("missing marmish-core compiler metadata")?;
    run(
        root,
        "rustc",
        &[
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            "--crate-name",
            "frame_consumer",
            "-L",
            &format!(
                "dependency={}",
                Path::new(&path).parent().unwrap().display()
            ),
            "--extern",
            &format!("marmish_core={path}"),
            "-o",
            tmp.path().join("consumer.rmeta").to_str().unwrap(),
            "-",
        ],
        Some(probe.as_bytes()),
    )?;
    Ok(json!({
        "compiler": compiler,
        "source": project["framing_source"],
        "consumer": probe,
        "compiledPackages": compiled,
        "status": "type-checked",
        "scope": "public framing methods, named errors and limit types"
    }))
}
