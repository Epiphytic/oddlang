//! rustc --edition 2024 companion-audit.rs -o /tmp/companion-audit
//! companion-audit BASELINE_TSV COMPANION_CHECKOUT COMPANION_COMMIT ORIGINAL_CHECKOUT
//! Reads immutable Git blobs. Output is tab-separated; source anchors are one-based.
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path},
    process::{Command, Stdio},
    thread,
};

const BASE: &str = "51064fa44124b811fdfbe1329cb62de4097eb8de";
const ORIGINAL: &str = "af54339d250290490d62d4475a77539b7e739071";

fn git(root: &str, args: &[&str]) -> String {
    let r = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8(r.stdout).unwrap()
}
type Tree = BTreeMap<String, (String, String)>;
fn tree(root: &str, revision: &str) -> Tree {
    git(root, &["ls-tree", "-r", revision])
        .lines()
        .filter_map(|line| {
            let (meta, path) = line.split_once('\t')?;
            let mut parts = meta.split_whitespace();
            let mode = parts.next()?;
            if !mode.starts_with("100") {
                return None;
            }
            Some((path.into(), (mode.into(), parts.nth(1)?.into())))
        })
        .collect()
}
fn ext(p: &str) -> &str {
    Path::new(p)
        .extension()
        .and_then(|p| p.to_str())
        .unwrap_or("")
}
fn script(p: &str) -> bool {
    matches!(
        ext(p),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "py"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "mts"
            | "cts"
            | "ps1"
            | "rb"
            | "pl"
            | "jq"
            | "go"
            | "c"
            | "h"
    )
}
fn role(p: &str, same: bool, upstream: bool) -> &'static str {
    if same {
        return "inherited-original";
    }
    if upstream {
        return match p {
            "scripts/install.sh" => "generated-release-installer",
            "tools/code-ontology/index.py" => "adapted-original-indexer",
            _ => "modified-original",
        };
    }
    match p {
        "verification/whole/core-node/check.mjs"
        | "verification/whole/gap-range-endpoint/typescript/check.cjs"
        | "verification/whole/gap-range-endpoint/typescript/endpoint.ts" => {
            "native-target-consumer"
        }
        "verification/whole/engine-limits/deny_darwin.c"
        | "verification/whole/basic-command-interop/prepare_journal_fixture.c" => {
            "native-observation-fixture"
        }
        p if p.ends_with("/workspace_compat.py") => "project-python-caller-adapter",
        _ => "project-tooling",
    }
}
// Inspect all file headers, including extensionless scripts, from the same revision as the inventory.
fn read_blobs(root: &str, files: &Tree, mut visit: impl FnMut(&str, &[u8])) {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let hashes: Vec<_> = files.values().map(|(_, h)| h.clone()).collect();
    let writer = thread::spawn(move || {
        for h in hashes {
            writeln!(input, "{h}").unwrap()
        }
    });
    let mut input = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let mut data = Vec::new();
    for (path, (_, hash)) in files {
        line.clear();
        input.read_line(&mut line).unwrap();
        let mut words = line.split_whitespace();
        assert_eq!(words.next(), Some(hash.as_str()));
        assert_eq!(words.next(), Some("blob"));
        data.resize(words.next().unwrap().parse().unwrap(), 0);
        input.read_exact(&mut data).unwrap();
        let mut newline = [0];
        input.read_exact(&mut newline).unwrap();
        assert_eq!(newline, [b'\n']);
        visit(path, &data);
    }
    writer.join().unwrap();
    assert!(child.wait().unwrap().success());
}
fn local(dir: &Path, token: &str) -> String {
    let mut out = std::path::PathBuf::new();
    for item in dir.join(token).components() {
        match item {
            Component::Normal(v) => out.push(v),
            Component::ParentDir => {
                if !out.pop() {
                    return String::new();
                }
            }
            Component::CurDir => {}
            _ => return String::new(),
        }
    }
    out.to_str().unwrap().to_owned()
}
fn main() {
    let [baseline, repo, revision, original]: [String; 4] = env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .expect("expected four arguments");
    let current = tree(&repo, &revision);
    let original_tree = tree(&original, ORIGINAL);
    let old = tree(&repo, BASE);
    let mut previous = BTreeMap::new();
    for row in fs::read_to_string(baseline).unwrap().lines().skip(1) {
        let cols: Vec<_> = row.split('\t').collect();
        if cols[0] != "marmish-oddlang" {
            continue;
        }
        assert_eq!(old[cols[1]].1, cols[2]);
        previous.insert(cols[1].to_owned(), cols[2].to_owned());
    }
    let mut paths: BTreeSet<String> = previous.keys().cloned().collect();
    let mut sources = BTreeMap::new();
    read_blobs(&repo, &current, |path, bytes| {
        if script(path)
            || current[path].0 == "100755"
            || bytes.starts_with(b"#!/")
            || bytes.starts_with(b"#! /")
        {
            paths.insert(path.into());
        }
        if !path.starts_with("docs/ontology/")
            && (script(path) || matches!(ext(path), "rs" | "md" | "toml" | "yml" | "yaml"))
        {
            if let Ok(s) = std::str::from_utf8(bytes) {
                sources.insert(path.to_owned(), s.to_owned());
            }
        }
    });
    let live: BTreeSet<_> = paths
        .iter()
        .filter(|p| current.contains_key(*p))
        .cloned()
        .collect();
    let mut basenames: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for p in &live {
        basenames
            .entry(Path::new(p).file_name().unwrap().to_str().unwrap())
            .or_default()
            .push(p);
    }
    let mut suffixes = BTreeMap::<String, Vec<&str>>::new();
    for path in &live {
        let parts: Vec<_> = path.split('/').collect();
        for start in 0..parts.len().saturating_sub(1) {
            suffixes
                .entry(parts[start..].join("/"))
                .or_default()
                .push(path);
        }
    }
    let mut references = BTreeMap::<String, BTreeSet<(String, usize)>>::new();
    for (source, code) in sources {
        let dir = Path::new(&source).parent().unwrap();
        let mut seen = BTreeSet::new();
        for (line_no, line) in code.lines().enumerate() {
            let mut targets = BTreeSet::new();
            for token in line.split(|c: char| !c.is_ascii_alphanumeric() && !"_./-".contains(c)) {
                if token.is_empty() {
                    continue;
                }
                let token = token.strip_prefix("./").unwrap_or(token);
                if let Some(paths) = suffixes.get(token)
                    && let [path] = paths.as_slice()
                {
                    targets.insert((*path).to_owned());
                }
                if live.contains(token) {
                    targets.insert(token.to_owned());
                }
                if token.contains('/') {
                    let relative = local(dir, token);
                    if live.contains(&relative) {
                        targets.insert(relative);
                    }
                } else if let Some(matches) = basenames.get(token) {
                    if let [p] = matches.as_slice() {
                        targets.insert((*p).to_owned());
                    } else if !line.contains("parent")
                        && !line.contains(" / ")
                        && !line.contains("join(")
                    {
                        let relative = local(dir, token);
                        if live.contains(&relative) {
                            targets.insert(relative);
                        }
                    }
                }
            }
            if line.contains("unittest discover -s") {
                if let Some(directory) = line
                    .split("unittest discover -s ")
                    .nth(1)
                    .and_then(|s| s.split_whitespace().next())
                {
                    let prefix = format!("{directory}/test_");
                    targets.extend(
                        live.iter()
                            .filter(|p| p.starts_with(&prefix) && p.ends_with(".py"))
                            .cloned(),
                    );
                }
            }
            if ext(&source) == "py" {
                if let Some(module) = line
                    .trim()
                    .strip_prefix("from ")
                    .or_else(|| line.trim().strip_prefix("import "))
                    .and_then(|s| s.split_whitespace().next())
                {
                    let relative = local(dir, &format!("{}.py", module.trim_end_matches(',')));
                    if live.contains(&relative) {
                        targets.insert(relative);
                    }
                }
            }
            for p in targets {
                if source != p && seen.insert(p.clone()) {
                    references
                        .entry(p)
                        .or_default()
                        .insert((source.clone(), line_no + 1));
                }
            }
        }
    }
    println!(
        "path\tbaselineBlob\tcurrentBlob\tupstreamSamePathBlob\tstatus\trole\tmode\tremovalCommit\tcodeReferences\tdocReferences"
    );
    for p in paths {
        let base = previous.get(&p).map(String::as_str).unwrap_or("-");
        let up = original_tree
            .get(&p)
            .map(|(_, h)| h.as_str())
            .unwrap_or("-");
        let Some((mode, hash)) = current.get(&p) else {
            let removal = git(
                &repo,
                &[
                    "log",
                    "-1",
                    "--diff-filter=D",
                    "--format=%H",
                    &format!("{BASE}..{revision}"),
                    "--",
                    &p,
                ],
            );
            assert!(!removal.trim().is_empty(), "missing removal {p}");
            println!(
                "{p}\t{base}\t-\t{up}\tremoved\t-\t-\t{}\t-\t-",
                removal.trim()
            );
            continue;
        };
        let status = if base == "-" {
            "outside-baseline"
        } else if base == hash {
            "same-blob"
        } else {
            "changed-blob"
        };
        let role = role(&p, up == hash, up != "-");
        let mut code = Vec::new();
        let mut docs = Vec::new();
        for (caller, n) in references.get(&p).into_iter().flatten() {
            let field = if ext(caller) == "md" {
                &mut docs
            } else {
                &mut code
            };
            field.push(format!("{caller}:{n}"));
        }
        let field = |items: Vec<String>| {
            if items.is_empty() {
                "-".into()
            } else {
                items.join(",")
            }
        };
        println!(
            "{p}\t{base}\t{hash}\t{up}\t{status}\t{role}\t{mode}\t-\t{}\t{}",
            field(code),
            field(docs)
        );
    }
}
