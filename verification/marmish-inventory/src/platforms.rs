use crate::*;
use regex::Regex;
use syntax::{Syntax, Workflows};

pub fn denominators(
    symbols: &[Value],
    files: &Map,
    platforms: &[String],
    profiles: &[String],
) -> Result<Value> {
    let mut by_kind = BTreeMap::new();
    let mut by_target: BTreeMap<String, Counts> = BTreeMap::new();
    let mut by_class: BTreeMap<String, Counts> = ["retained-native", "imported", "unsupported"]
        .into_iter()
        .map(|k| (k.to_string(), Counts::new()))
        .collect();
    let mut sites = 0;
    for sym in symbols {
        if let Some(counts) = by_class.get_mut(string(&sym["classification"])?) {
            let kind = string(&sym["kind"])?;
            sites += 1;
            increment(counts, kind);
            increment(&mut by_kind, kind);
            for target in target_names(&sym["targets"])? {
                increment(by_target.entry(target).or_default(), kind);
            }
        }
    }
    let gaps: Vec<_> = files
        .iter()
        .filter(|(p, f)| {
            p.starts_with("crates/")
                && matches!(
                    f["parseStatus"].as_str(),
                    Some("recovered" | "parsed_with_omissions")
                )
        })
        .map(|(p, _)| p)
        .collect();
    let mut compiled = Map::new();
    for p in platforms {
        let unit = json!({
            "unit": "compiler-resolved post-cfg, post-expansion first-party runtime declarations for this target/profile",
            "status": "unknown",
            "count": null,
            "reason": "no complete compiler item/reachability inventory for this configuration"
        });
        let configs: Map = profiles
            .iter()
            .map(|name| (name.clone(), unit.clone()))
            .collect();
        compiled.insert(p.clone(), json!(configs));
    }
    Ok(json!({
        "lexicalUnion": {
            "unit": "one distinct upstream lexical declaration ID, including members and containers, across the union of source configurations",
            "scope": "workspace runtime candidates, including declarations whose production reachability is uncertain",
            "observedSites": sites,
            "byKind": by_kind,
            "byClassification": by_class,
            "byCargoTarget": by_target,
            "targetAggregation": "one site can appear under several Cargo targets; sum observedSites only over distinct IDs",
            "incompleteParsePaths": gaps,
            "additionalUnknownSites": null,
            "configurationCoverage": "static module candidates; inactive branches can be present; no macro expansion"
        },
        "compiledRuntimeUnits": compiled
    }))
}
fn target_names(v: &Value) -> Result<Vec<String>> {
    let mut names = strings(v)?;
    if names.is_empty() {
        names.push("unresolved-target".into());
    }
    Ok(names)
}
fn all(values: &[Option<bool>]) -> Option<bool> {
    if values.contains(&Some(false)) {
        Some(false)
    } else if values.contains(&None) {
        None
    } else {
        Some(true)
    }
}
fn predicate(expr: &str, cfg: &Set) -> Option<bool> {
    if cfg.contains(expr) {
        return Some(true);
    }
    if ["target_os=", "target_arch=", "feature="]
        .iter()
        .any(|p| expr.starts_with(p))
        || matches!(expr, "test" | "debug_assertions" | "unix" | "windows")
    {
        return Some(false);
    }
    let (op, inner) = expr.strip_suffix(')')?.split_once('(')?;
    let (mut depth, mut start) = (0, 0);
    let mut values = Vec::new();
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                values.push(predicate(&inner[start..i], cfg));
                start = i + 1;
            }
            _ => {}
        }
    }
    values.push(predicate(&inner[start..], cfg));
    match op {
        "not" if values.len() == 1 => values[0].map(|b| !b),
        "all" => all(&values),
        "any" => all(&values.iter().map(|v| v.map(|b| !b)).collect::<Vec<_>>()).map(|b| !b),
        _ => None,
    }
}
pub fn eligible<'a>(attrs: impl IntoIterator<Item = &'a String>, cfg: &Set) -> Option<bool> {
    let mut states = Vec::new();
    for raw in attrs {
        let a: String = raw.split_whitespace().collect();
        if let Some(expr) = a.strip_prefix("#[cfg(") {
            states.push(expr.strip_suffix(")]").and_then(|x| predicate(x, cfg)));
        } else if a.starts_with("#[cfg_attr(")
            && a != "#[cfg_attr(test,track_caller)]"
            && a != "#[cfg_attr(not(debug_assertions),allow(dead_code))]"
        {
            states.push(None);
        }
    }
    all(&states)
}
pub type Applicability = BTreeMap<String, Vec<Value>>;
pub fn tests(
    syntax: &BTreeMap<String, Syntax>,
    symbols: &[Value],
    files: &Map,
    targets: &Value,
    cfgs: &BTreeMap<String, Set>,
    profiles: &[String],
) -> Result<(Map, Applicability)> {
    let mut loc = BTreeMap::new();
    for sym in symbols.iter().filter(|s| s["kind"] == "function_item") {
        loc.insert(
            (
                string(&sym["path"])?,
                number(&sym["start_line"])?,
                string(&sym["name"])?,
            ),
            &sym["id"],
        );
    }
    let mut catalog = Map::new();
    let mut selected: Applicability = cfgs.keys().map(|t| (t.clone(), vec![])).collect();
    for (path, src) in syntax {
        for test in &src.tests {
            let line = number(&test["line"])?;
            let name = string(&test["name"])?;
            let site = format!("{path}:{line}:{name}");
            let ignores: Vec<_> = strings(&test["attributes"])?
                .into_iter()
                .filter(|a| a.starts_with("#[ignore"))
                .collect();
            let gate = if ignores.is_empty() {
                "ordinary"
            } else {
                "ignored"
            };
            let attrs = src.attrs(line);
            let contexts: &[Value] = files[path]["cargoContexts"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut tids: Set = contexts
                .iter()
                .map(|c| string(&c["target"]).map(str::to_owned))
                .collect::<Result<_>>()?;
            catalog.insert(
                site.clone(),
                json!({
                    "path": path,
                    "line": line,
                    "name": name,
                    "symbolId": loc.get(&(path.as_str(),line,name)),
                    "gate": gate,
                    "ignoreAttributes": ignores,
                    "targets": tids
                }),
            );
            if tids.is_empty() {
                tids.insert("unresolved-target".into());
            }
            for (platform, builtin) in cfgs {
                for tid in &tids {
                    let (mut applicable, mut unknown) = (Vec::new(), Vec::new());
                    for profile in profiles {
                        let mut cfg = builtin.clone();
                        cfg.extend(["test".to_string(), "debug_assertions".to_string()]);
                        if profile != "default" {
                            let (krate, feature) = profile
                                .split_once('/')
                                .context("expected crate/feature profile")?;
                            if targets[tid]["crate"] == krate {
                                cfg.insert(format!("feature=\"{feature}\""));
                            }
                        }
                        let mut states = Vec::new();
                        for c in contexts.iter().filter(|c| c["target"] == *tid) {
                            let extra = strings(&c["attributes"])?;
                            states.push(eligible(attrs.iter().chain(&extra), &cfg));
                        }
                        if states.contains(&Some(true)) {
                            applicable.push(profile);
                        } else if states.is_empty() || states.contains(&None) {
                            unknown.push(profile);
                        }
                    }
                    if !applicable.is_empty() || !unknown.is_empty() {
                        selected.get_mut(platform).unwrap().push(json!({
                            "test": site,
                            "cargoTarget": tid,
                            "gate": gate,
                            "profiles": applicable,
                            "uncertainProfiles": unknown
                        }));
                    }
                }
            }
        }
    }
    Ok((catalog, selected))
}
pub fn packaging(
    platform: &str,
    files: &Map,
    workflows: &Workflows,
    targets: &Value,
) -> Result<Value> {
    let mut sources: Set = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates/marmish-native/build.rs",
        "crates/marmish-native/Cargo.toml",
        "tools/marmish_native_licenses.py",
        "scripts/v02-inspect-native.sh",
        "crates/marmish-native/src/native_release.rs",
        "crates/marmish-native/src/native_manifest.rs",
        "scripts/install-native.sh",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let mut inputs = vec![];
    let mut recipes = vec![];
    for binary in [
        "marmish",
        "marmishd",
        "marmish-worker",
        "marmish-release-tool",
    ] {
        let id = format!("marmish-native:bin:{binary}");
        sources.insert(string(&targets[&id]["path"])?.into());
        if binary != "marmish-release-tool" {
            inputs.push(json!({
                "path": format!("package-input/{binary}"),
                "producer": id,
                "sha256": null,
                "byteEvidence": "unobserved-final-package-input"
            }));
        }
    }
    inputs.push(json!({
        "path": "THIRD-PARTY-LICENSES",
        "producer": "tools/marmish_native_licenses.py",
        "sha256": null,
        "byteEvidence": "unobserved-final-package-input"
    }));
    for path in [
        ".github/workflows/v02-release.yml",
        ".github/workflows/streaming-candidate.yml",
    ] {
        for (name, job) in workflows.get(path).context("missing packaging workflow")? {
            let Some(step) = job["steps"].as_array().into_iter().flatten().find(|s| {
                s["run"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("marmish-release-tool package --input-dir")
            }) else {
                continue;
            };
            let row = array(&job["strategy"]["matrix"]["include"])?
                .iter()
                .find(|r| r["target"] == platform)
                .context("missing packaging platform")?;
            let body = string(&step["run"])?;
            ensure!(
                body.contains("for binary in marmish marmishd marmish-worker;")
                    && body.contains("--licenses \"$RUNNER_TEMP/THIRD-PARTY-LICENSES\""),
                "unaccounted packaging input in {path}"
            );
            let mut record = json!({
                "workflow": path,
                "workflowSha256": files[path]["sha256"],
                "job": name,
                "step": step["name"],
                "stepRunSha256": digest(body.as_bytes())
            });
            record.as_object_mut().unwrap().extend(object(row)?.clone());
            recipes.push(record);
            sources.insert(path.into());
        }
    }
    ensure!(
        recipes.len() == 2,
        "expected release and streaming packaging recipes"
    );
    let tracked: Map = sources
        .into_iter()
        .map(|p| {
            Ok((
                p.clone(),
                files.get(&p).with_context(|| format!("missing {p}"))?["sha256"].clone(),
            ))
        })
        .collect::<Result<_>>()?;
    Ok(json!({
        "recipes": recipes,
        "trackedRecipeInputs": tracked,
        "finalInputs": inputs,
        "packager": "marmish-native:bin:marmish-release-tool"
    }))
}
pub fn workflow_tests(workflows: &Workflows) -> Result<Vec<Value>> {
    let pattern = Regex::new(r"\bcargo\s+test\b|\bunittest\b|\bnode\s+--test\b|--ignored\b")?;
    let mut out = vec![];
    for (path, jobs) in workflows {
        for (id, job) in jobs {
            for (i, step) in job["steps"].as_array().into_iter().flatten().enumerate() {
                let cmd = step["run"].as_str().unwrap_or_default();
                if pattern.is_match(cmd) {
                    out.push(json!({
                        "workflow": path,
                        "job": id,
                        "step": step.get("name").cloned().unwrap_or_else(||json!(format!("step-{}",i+1))),
                        "runsOn": job["runs-on"],
                        "matrix": job["strategy"]["matrix"],
                        "jobIf": job["if"],
                        "stepIf": step["if"],
                        "explicitIgnored": cmd.contains("--ignored"),
                        "stepRunSha256": digest(cmd.as_bytes())
                    }));
                }
            }
        }
    }
    Ok(out)
}
