use super::*;
pub fn index(
    upstream: &Path,
    dest: &Path,
    evidence: &Path,
    inventory: &Value,
    releases: &Path,
) -> Result<Value> {
    let timestamp = line(git(upstream).args(["show", "-s", "--format=%aI", PIN]))?;
    let env = [
        ("GIT_AUTHOR_NAME", "Oddlang qualification"),
        ("GIT_COMMITTER_NAME", "Oddlang qualification"),
        ("GIT_AUTHOR_EMAIL", "qualification@oddlang.invalid"),
        ("GIT_COMMITTER_EMAIL", "qualification@oddlang.invalid"),
        ("GIT_AUTHOR_DATE", &timestamp),
        ("GIT_COMMITTER_DATE", &timestamp),
    ];
    let cmd = || {
        let mut c = git(dest);
        c.envs(env);
        c
    };
    output(cmd().args(["init", "-q", "--initial-branch=candidate", "--template="]))?;
    for tag in fs::read_to_string(releases.join("ontology-tags.lock"))?.lines() {
        let (id, tag) = tag.split_once(' ').context("invalid ontology tag lock")?;
        ensure!(
            line(git(upstream).args(["rev-parse", tag]))? == id,
            "upstream ontology tag changed: {tag}"
        );
        output(
            cmd()
                .args(["fetch", "-q", "--no-tags", "--no-write-fetch-head"])
                .arg(upstream)
                .arg(format!("{tag}:{tag}")),
        )?;
    }
    output(cmd().args(["add", "-f", "--all"]))?;
    let staged = output(cmd().args(["ls-files", "-s", "-z"]))?;
    let mut entries = BTreeMap::new();
    for row in std::str::from_utf8(&staged)?.split_terminator('\0') {
        let (header, p) = row.split_once('\t').context("stage path")?;
        let fields: Vec<_> = header.split_whitespace().collect();
        ensure!(
            fields.len() == 3
                && fields[2] == "0"
                && entries.insert(p, (fields[0], fields[1])).is_none(),
            "invalid staged entry"
        );
    }
    let manifest = read(&evidence.join("project.toml"))?;
    let project: Value = toml::from_str(std::str::from_utf8(&manifest)?)?;
    let files = object(&inventory["files"])?;
    let rows = array(&project["files"])?;
    reconstruction::check_source_rows(
        rows,
        files,
        &set(&[
            "crates/marmish-core/src/frame.rs",
            "crates/marmish-core/src/lib.rs",
        ]),
    )?;
    ensure!(
        entries.len() == rows.len(),
        "assembled tree membership differs"
    );
    let mut generated = Map::new();
    for row in rows {
        let p = text(&row["path"])?;
        let (mode, blob) = entries
            .get(p)
            .with_context(|| format!("missing assembled entry {p}"))?;
        ensure!(
            row["sha256"] == digest(&read(&dest.join(p))?) && row["mode"] == *mode,
            "assembled bytes or mode disagree with manifest: {p}"
        );
        let old = &files[p]["gitBlob"];
        if row["producer"] == "generated" {
            ensure!(old != *blob, "expected distinct generated projection: {p}");
            generated.insert(
                p.into(),
                json!({"originalBlob":old,"candidateBlob":blob,"sha256":row["sha256"]}),
            );
        } else {
            ensure!(old == *blob, "retained Git entry changed: {p}");
        }
    }
    let staged_tree = line(cmd().arg("write-tree"))?;
    let venv = tempfile::Builder::new()
        .prefix("oddlang-index-venv-")
        .tempdir()?;
    let index = |args: &[&str]| {
        output(
            offline::command()?
                .current_dir(dest)
                .envs(env)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("UV_PROJECT_ENVIRONMENT", venv.path().join("venv"))
                .env("UV_OFFLINE", "1")
                .env("UV_PYTHON_DOWNLOADS", "never")
                .args([
                    "uv",
                    "run",
                    "--locked",
                    "--offline",
                    "--python",
                    "3.12",
                    "--project",
                    "tools/code-ontology",
                    "python",
                    "tools/code-ontology/index.py",
                ])
                .args(args),
        )
    };
    fs::write(evidence.join("ontology-update.txt"), index(&["update"])?)?;
    let changed: Set = line(cmd().args(["diff", "--name-only"]))?
        .lines()
        .map(str::to_owned)
        .collect();
    ensure!(
        changed
            == set(&[
                "docs/ontology/code_references.jsonl",
                "docs/ontology/source_files.jsonl",
                "docs/ontology/symbols.jsonl"
            ])
            && output(cmd().args(["ls-files", "--others", "--exclude-standard"]))?.is_empty(),
        "unexpected ontology update paths"
    );
    output(cmd().args(["add", "--update"]))?;
    output(cmd().args(["commit", "-q", "-m"]).arg(format!(
        "Qualify regenerated Marmish framing and facade from {PIN}"
    )))?;
    fs::write(
        evidence.join("ontology-check.txt"),
        index(&["check", "--ref", "HEAD"])?,
    )?;
    ensure!(
        output(cmd().args(["status", "--porcelain"]))?.is_empty(),
        "immutable candidate check changed source tree"
    );
    let immutable = evidence.join("immutable");
    fs::create_dir(&immutable)?;
    fs::write(
        immutable.join("commit.raw"),
        output(cmd().args(["cat-file", "commit", "HEAD"]))?,
    )?;
    for p in &changed {
        fs::write(
            immutable.join(Path::new(p).file_name().unwrap()),
            output(cmd().arg("show").arg(format!("HEAD:{p}")))?,
        )?;
    }
    let result = json!({
    "assemblySha256":digest(&manifest),
    "sourceFiles":rows.len(),"retainedFiles":rows.len()-generated.len(),"generated":generated,
    "stagedAssemblyTree":staged_tree,"regeneratedOntology":changed,
    "candidateCommit":line(cmd().args(["rev-parse","HEAD"]))?,
    "candidateTree":line(cmd().args(["rev-parse","HEAD^{tree}"]))?
    });
    write_json(&evidence.join("result.json"), &result)?;
    Ok(result)
}
