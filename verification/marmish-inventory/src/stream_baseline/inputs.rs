use super::*;
use regex::Regex;

pub fn dep_info(raw: &str) -> Result<(Vec<String>, Map)> {
    let joined = raw.replace("\\\n", "");
    let mut lines = joined.lines();
    let (_, inputs) = lines
        .next()
        .context("empty dep-info")?
        .split_once(": ")
        .context("invalid dep-info")?;
    let mut paths = Vec::new();
    let mut word = String::new();
    let mut chars = inputs.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            word.push(chars.next().context("incomplete dep-info escape")?);
        } else if c.is_whitespace() {
            if !word.is_empty() {
                paths.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        paths.push(word);
    }
    let mut env = Map::new();
    for line in lines.filter_map(|l| l.strip_prefix("# env-dep:")) {
        let (key, value) = line
            .split_once('=')
            .map(|(k, v)| (k, json!(v)))
            .unwrap_or((line, Value::Null));
        ensure!(
            env.insert(key.into(), value).is_none(),
            "duplicate rustc environment dependency {key}"
        );
    }
    Ok((paths, env))
}
#[derive(Debug)]
pub struct Invocation {
    pub env: Vars,
    pub argv: Vec<String>,
    pub words: Vec<String>,
}
pub fn commands(raw: &str) -> Result<Vec<Invocation>> {
    let mut out = vec![];
    let mut pending: Option<String> = None;
    for line in raw.lines() {
        if let Some(text) = &mut pending {
            text.push('\n');
            text.push_str(line);
        } else if line.starts_with(char::is_whitespace)
            && let Some(rest) = line.trim_start().strip_prefix("Running `")
        {
            pending = Some(rest.into());
        }
        if let Some(s) = pending.as_ref().and_then(|s| s.strip_suffix('`'))
            && let Some(words) = shlex::split(s)
        {
            let mut env = Vars::new();
            let mut start = 0;
            for word in &words {
                let Some((name, val)) = word.split_once('=') else {
                    break;
                };
                let mut chars = name.bytes();
                if !chars
                    .next()
                    .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
                    || !chars.all(|b| b == b'_' || b.is_ascii_alphanumeric())
                {
                    break;
                }
                env.insert(name.into(), val.into());
                start += 1;
            }
            out.push(Invocation {
                env,
                argv: words[start..].to_vec(),
                words,
            });
            pending = None;
        }
    }
    ensure!(pending.is_none(), "incomplete verbose Cargo command");
    Ok(out)
}
pub type Producers = BTreeMap<PathBuf, String>;
// Cargo renders workspace package IDs as file URLs, including percent-encoded paths.
fn workspace_url(root: &Path) -> String {
    use std::fmt::Write;
    let mut url = String::from("path+file://");
    for b in root.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(b) {
            url.push(*b as char);
        } else {
            write!(url, "%{b:02X}").expect("string");
        }
    }
    url.push('/');
    url
}
pub struct Roots<'a> {
    pub source: &'a Path,
    pub target: &'a Path,
    pub home: &'a Path,
    pub vendor: Option<&'a Path>,
    pub tools: &'a Path,
    pub sdk: &'a Path,
    pub declared: &'a Map,
}
impl Roots<'_> {
    pub fn norm(&self, value: &str, with_vendor: bool) -> String {
        let mut roots = vec![
            (self.source, LOGICAL),
            (self.target, "$TARGET"),
            (self.home, "$CARGO_HOME"),
            (self.tools, "$TOOLCHAIN"),
            (self.sdk, "$SDK"),
        ];
        if with_vendor && let Some(v) = self.vendor {
            roots.push((v, "$VENDOR"));
        }
        for (root, name) in roots {
            if let Ok(rel) = Path::new(value).strip_prefix(root) {
                return if rel.as_os_str().is_empty() {
                    name.into()
                } else {
                    format!("{name}/{}", rel.display())
                };
            }
        }
        value.into()
    }
    pub fn observed(&self, path: &Path, producers: &Producers) -> Result<Value> {
        let path = self
            .source
            .join(path)
            .canonicalize()
            .with_context(|| format!("missing compiler input {}", path.display()))?;
        let mut roots = vec![
            (self.source.to_owned(), "workspace"),
            (self.target.to_owned(), "build"),
            (self.home.join("registry/src"), "registry"),
            (self.home.join("git/checkouts"), "git-dependency"),
            (self.tools.to_owned(), "toolchain"),
            (self.sdk.to_owned(), "sdk"),
        ];
        if let Some(v) = self.vendor {
            roots.insert(0, (v.into(), "registry"));
        }
        for (ancestor, label) in roots {
            let Ok(rel) = path.strip_prefix(&ancestor) else {
                continue;
            };
            let relative = rel.to_str().context("non-UTF8 input")?;
            let relative = match label {
                "workspace" => {
                    ensure!(
                        self.declared.contains_key(relative),
                        "undeclared compiler input {relative}"
                    );
                    relative.into()
                }
                "registry" if self.vendor != Some(ancestor.as_path()) => relative
                    .split_once('/')
                    .context("missing registry directory")?
                    .1
                    .into(),
                "build" => {
                    let (dir, id) = producers
                        .iter()
                        .find(|(d, _)| path.starts_with(d))
                        .with_context(|| {
                            format!("no Cargo build script produced {}", path.display())
                        })?;
                    format!("{id}/{}", path.strip_prefix(dir)?.display())
                }
                _ => relative.to_owned(),
            };
            return Ok(json!([label, relative, hash_file(&path)?]));
        }
        bail!(
            "compiler dep-info escaped declared roots: {}",
            path.display()
        )
    }
    pub fn producers(
        &self,
        messages: &[Value],
        commands: &[Invocation],
        vars: &Vars,
    ) -> Result<(Producers, Vec<Value>, Map)> {
        let scripts: Vec<_> = messages
            .iter()
            .filter(|m| m["reason"] == "build-script-executed")
            .collect();
        let mut producers = Producers::new();
        for script in &scripts {
            let dir = PathBuf::from(text(&script["out_dir"])?);
            ensure!(
                dir.starts_with(self.target),
                "build script OUT_DIR escaped target"
            );
            let id =
                text(&script["package_id"])?.replace(&workspace_url(self.source), "workspace:");
            ensure!(
                producers.insert(dir, id).is_none(),
                "duplicate build script OUT_DIR"
            );
        }
        let mut runs = BTreeMap::new();
        for invocation in commands {
            if let Some(dir) = invocation.env.get("OUT_DIR").map(PathBuf::from)
                && producers.contains_key(&dir)
                && invocation.argv.len() == 1
                && invocation.argv[0].contains(self.target.to_str().unwrap())
            {
                ensure!(
                    runs.insert(dir, invocation).is_none(),
                    "duplicate build script run"
                );
            }
        }
        ensure!(
            producers.keys().eq(runs.keys()),
            "some Cargo build-script executions lack verbose invocations"
        );
        let mut facts = vec![];
        let mut hashes = Map::new();
        let directive = Regex::new(r"^cargo::?([^=]+)=(.*)$")?;
        for script in scripts {
            let dir = Path::new(text(&script["out_dir"])?);
            let invocation = runs[dir];
            let id = &producers[dir];
            let manifest = Path::new(
                invocation
                    .env
                    .get("CARGO_MANIFEST_DIR")
                    .context("missing CARGO_MANIFEST_DIR")?,
            )
            .join("Cargo.toml");
            let raw = fs::read(dir.parent().context("missing script root")?.join("output"))?;
            hashes.insert(id.clone(),json!({"stdout":digest(&raw),"executable":hash_file(Path::new(&invocation.argv[0]))?}));
            let mut files = vec![];
            let mut env = Map::new();
            let mut directives = vec![];
            for m in std::str::from_utf8(&raw)?
                .lines()
                .filter_map(|line| directive.captures(line))
            {
                let (key, value) = (&m[1], &m[2]);
                match key {
                    "rerun-if-changed" => {
                        let p = manifest.parent().unwrap().join(value);
                        if p.is_dir() {
                            for f in walk(&p)? {
                                if f.is_file() {
                                    files.push(self.observed(&f, &producers)?);
                                }
                            }
                        } else if p.is_file() {
                            files.push(self.observed(&p, &producers)?);
                        } else {
                            files.push(json!(["missing-declaration", value, null]));
                        }
                    }
                    "rerun-if-env-changed" => {
                        env.insert(
                            value.into(),
                            json!(
                                invocation
                                    .env
                                    .get(value)
                                    .or_else(|| vars.get(value))
                                    .map(|v| self.norm(v, true))
                            ),
                        );
                    }
                    _ => directives.push(json!([key, self.norm(value, true)])),
                }
            }
            sort_json(&mut files);
            sort_json(&mut directives);
            facts.push(json!({"packageId":id,"manifest":self.observed(&manifest,&producers)?,"declaredRerunFiles":files,"declaredRerunEnvironment":env,"directives":directives,"declaredCfg":sorted(&script["cfgs"])?,"linkedLibraries":sorted(&script["linked_libs"])?,"coverage":"Cargo-declared rerun inputs and outputs"}));
        }
        facts.sort_by_cached_key(|f| f["packageId"].as_str().unwrap().to_owned());
        Ok((producers, facts, hashes))
    }
    pub fn inventory(
        &self,
        commands: &[Invocation],
        producers: &Producers,
    ) -> Result<(Vec<Value>, Vec<Value>)> {
        let rustc = self.tools.join("bin/rustc");
        let rustdoc = self.tools.join("bin/rustdoc");
        let mut records = vec![];
        let mut documents = vec![];
        let mut depfiles = BTreeSet::new();
        for invocation in commands {
            let Some((i, compiler)) = invocation
                .words
                .iter()
                .enumerate()
                .find(|(_, w)| Path::new(w) == rustc || Path::new(w) == rustdoc)
            else {
                continue;
            };
            let args = &invocation.words[i + 1..];
            let flag = |name: &str| {
                args.windows(2)
                    .filter(|w| w[0] == name)
                    .map(|w| w[1].clone())
                    .collect::<Vec<_>>()
            };
            if Path::new(compiler) == rustdoc {
                documents.push(json!({"root":args.iter().find(|a|a.ends_with(".rs")).context("missing rustdoc source")?,"scope":"rustdoc invocation; stable rustc dep-info is unavailable"}));
                continue;
            }
            let krate = flag("--crate-name")
                .into_iter()
                .next()
                .context("missing crate name")?;
            let directory = flag("--out-dir")
                .into_iter()
                .next()
                .context("missing rustc output directory")?;
            let extra = flag("-C")
                .iter()
                .find_map(|v| v.strip_prefix("extra-filename=").map(str::to_owned))
                .unwrap_or_default();
            let depfile = PathBuf::from(directory).join(format!("{krate}{extra}.d"));
            ensure!(
                depfile.is_file(),
                "rustc invocation has no matching dep-info: {}",
                depfile.display()
            );
            ensure!(
                depfiles.insert(depfile.clone()),
                "repeated dep-info destination"
            );
            let (paths, environ) = dep_info(&fs::read_to_string(&depfile)?)?;
            let root = args
                .iter()
                .find(|a| paths.contains(a))
                .context("rustc invocation root did not appear in dep-info")?;
            let env: Map = environ
                .into_iter()
                .map(|(k, v)| (k, json!(v.as_str().map(|s| self.norm(s, false)))))
                .collect();
            let mut inputs = paths
                .iter()
                .map(|p| self.observed(Path::new(p), producers))
                .collect::<Result<Vec<_>>>()?;
            sort_json(&mut inputs);
            let mut aliases: Vec<_> = flag("--extern")
                .iter()
                .map(|v| v.split('=').next().unwrap().to_owned())
                .collect();
            aliases.sort();
            let mut cfg = flag("--cfg");
            cfg.sort();
            records.push(json!({"crate":krate,"root":self.observed(Path::new(root),producers)?,"test":args.iter().any(|a|a=="--test"),"sourcedCfg":cfg,"crateTypes":flag("--crate-type"),"externAliases":aliases,"inputs":inputs,"environment":env}));
        }
        ensure!(
            !records.is_empty(),
            "no stable rustc compiler invocations observed"
        );
        let disk: BTreeSet<_> = walk(self.target)?
            .into_iter()
            .filter(|p| {
                let rel = p.strip_prefix(self.target).unwrap();
                p.extension().is_some_and(|x| x == "d")
                    && !rel.iter().any(|x| x == "out")
                    && (rel.iter().any(|x| x == "build")
                        || p.parent().is_some_and(|p| p.ends_with("deps")))
            })
            .collect();
        ensure!(
            disk == depfiles,
            "unpaired rustc dep-info: {:?}",
            disk.symmetric_difference(&depfiles).collect::<Vec<_>>()
        );
        sort_json(&mut records);
        Ok((records, documents))
    }
}
