"""Source declaration denominators and per-platform static qualification inputs."""

from collections import Counter, defaultdict
from hashlib import sha256
import re


def denominators(symbols, files, platforms, profiles):
    sites = [
        s
        for s in symbols
        if s["classification"] in {"retained-native", "imported", "unsupported"}
    ]
    by_target = defaultdict(Counter)
    by_class = defaultdict(Counter)
    for s in sites:
        by_class[s["classification"]][s["kind"]] += 1
        for t in s["targets"] or ["unresolved-target"]:
            by_target[t][s["kind"]] += 1
    gaps = sorted(
        p
        for p, f in files.items()
        if p.startswith("crates/")
        and f.get("parseStatus") in {"recovered", "parsed_with_omissions"}
    )
    return {
        "lexicalUnion": {
            "unit": "one distinct upstream lexical declaration ID, including members and containers, across the union of source configurations",
            "scope": "workspace runtime candidates, including declarations whose production reachability is uncertain",
            "observedSites": len(sites),
            "byKind": Counter(s["kind"] for s in sites),
            "byClassification": {
                k: by_class[k] for k in ("retained-native", "imported", "unsupported")
            },
            "byCargoTarget": dict(by_target),
            "targetAggregation": "one site can appear under several Cargo targets; sum observedSites only over distinct IDs",
            "incompleteParsePaths": gaps,
            "additionalUnknownSites": None,
            "configurationCoverage": "static module candidates; inactive branches can be present; no macro expansion",
        },
        "compiledRuntimeUnits": {
            p: {
                f: {
                    "unit": "compiler-resolved post-cfg, post-expansion first-party runtime declarations for this target/profile",
                    "status": "unknown",
                    "count": None,
                    "reason": "no complete compiler item/reachability inventory for this configuration",
                }
                for f in profiles
            }
            for p in platforms
        },
    }


def combine_and(values):
    if False in values:
        return False
    return None if None in values else True


def predicate(expr, cfg):
    if expr in cfg:
        return True
    if expr.startswith(("target_os=", "target_arch=", "feature=")) or expr in {
        "test",
        "debug_assertions",
        "unix",
        "windows",
    }:
        return False
    match = re.fullmatch(r"(all|any|not)\((.*)\)", expr)
    if not match:
        return None
    depth, start, parts = 0, 0, []
    body = match[2]
    for i, c in enumerate(body):
        if c == "," and depth == 0:
            parts.append(body[start:i])
            start = i + 1
        depth += (c == "(") - (c == ")")
    parts.append(body[start:])
    values = [predicate(p, cfg) for p in parts]
    if match[1] == "not":
        return None if len(values) != 1 or values[0] is None else not values[0]
    if match[1] == "all":
        return combine_and(values)
    return True if True in values else (None if None in values else False)


def eligible(attrs, cfg):
    states = []
    for raw in attrs:
        attr = "".join(raw.split())
        if attr.startswith("#[cfg("):
            states.append(predicate(attr[len("#[cfg(") : -2], cfg))
        elif attr.startswith("#[cfg_attr(") and attr not in (
            "#[cfg_attr(test,track_caller)]",
            "#[cfg_attr(not(debug_assertions),allow(dead_code))]",
        ):
            states.append(None)
    return combine_and(states)


def test_inventory(syntax, symbols, files, targets, cfgs, profiles):
    by_location = {
        (s["path"], s["start_line"], s["name"]): s["id"]
        for s in symbols
        if s["kind"] == "function_item"
    }
    catalog = {}
    by_platform = {t: [] for t in cfgs}
    for path, src in syntax.items():
        for test in src["tests"]:
            line, name = test["line"], test["name"]
            site = f"{path}:{line}:{name}"
            direct = test["attributes"]
            ignores = [a for a in direct if a.startswith("#[ignore")]
            attrs = [
                a
                for start, end, aa in src["attributes"]
                if start <= line <= end
                for a in aa
            ]
            contexts = files[path].get("cargoContexts", [])
            catalog[site] = {
                "path": path,
                "line": line,
                "name": name,
                "symbolId": by_location.get((path, line, name)),
                "gate": "ignored" if ignores else "ordinary",
                "ignoreAttributes": ignores,
                "targets": sorted({c["target"] for c in contexts}),
            }
            for platform, builtin in cfgs.items():
                for tid in catalog[site]["targets"] or ["unresolved-target"]:
                    applicable, unknown = [], []
                    for profile in profiles:
                        cfg = (set(builtin) - {"debug_assertions"}) | {
                            "test",
                            "debug_assertions",
                        }
                        if profile != "default":
                            crate, feature = profile.split("/")
                            if (
                                tid != "unresolved-target"
                                and targets[tid]["crate"] == crate
                            ):
                                cfg.add(f'feature="{feature}"')
                        states = [
                            eligible([*attrs, *c["attributes"]], cfg)
                            for c in contexts
                            if c["target"] == tid
                        ]
                        if True in states:
                            applicable.append(profile)
                        elif None in states or not states:
                            unknown.append(profile)
                    if applicable or unknown:
                        by_platform[platform].append(
                            {
                                "test": site,
                                "cargoTarget": tid,
                                "gate": catalog[site]["gate"],
                                "profiles": applicable,
                                "uncertainProfiles": unknown,
                            }
                        )
    return catalog, by_platform


def package_inputs(platform, files, workflows, targets):
    records = []
    binaries = ("marmish", "marmishd", "marmish-worker")
    source_paths = {
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
    }
    for binary in (*binaries, "marmish-release-tool"):
        source_paths.add(targets[f"marmish-native:bin:{binary}"]["path"])
    for path in (
        ".github/workflows/v02-release.yml",
        ".github/workflows/streaming-candidate.yml",
    ):
        jobs = workflows[path]
        for job_name, job in jobs.items():
            steps = job.get("steps", [])
            package = next(
                (
                    s
                    for s in steps
                    if "marmish-release-tool package --input-dir" in s.get("run", "")
                ),
                None,
            )
            if package is None:
                continue
            matrix = job["strategy"]["matrix"]["include"]
            row = next(r for r in matrix if r["target"] == platform)
            body = package["run"]
            if not all(
                fragment in body
                for fragment in [
                    "for binary in marmish marmishd marmish-worker;",
                    '--licenses "$RUNNER_TEMP/THIRD-PARTY-LICENSES"',
                ]
            ):
                raise ValueError("unaccounted packaging input in " + path)
            records.append(
                {
                    "workflow": path,
                    "workflowSha256": files[path]["sha256"],
                    "job": job_name,
                    "step": package["name"],
                    "stepRunSha256": sha256(body.encode()).hexdigest(),
                    **row,
                }
            )
            source_paths.add(path)
    if len(records) != 2:
        raise ValueError("expected release and streaming packaging recipes")
    inputs = [
        {
            "path": "package-input/" + b,
            "producer": "marmish-native:bin:" + b,
            "sha256": None,
            "byteEvidence": "unobserved-final-package-input",
        }
        for b in binaries
    ]
    inputs.append(
        {
            "path": "THIRD-PARTY-LICENSES",
            "producer": "tools/marmish_native_licenses.py",
            "sha256": None,
            "byteEvidence": "unobserved-final-package-input",
        }
    )
    return {
        "recipes": records,
        "trackedRecipeInputs": {p: files[p]["sha256"] for p in sorted(source_paths)},
        "finalInputs": inputs,
        "packager": "marmish-native:bin:marmish-release-tool",
    }


def workflow_test_steps(workflows):
    steps = []
    for path, jobs in workflows.items():
        for job_id, job in jobs.items():
            for i, s in enumerate(job.get("steps", [])):
                cmd = s.get("run", "")
                if not re.search(
                    r"\bcargo\s+test\b|\bunittest\b|\bnode\s+--test\b|--ignored\b", cmd
                ):
                    continue
                steps.append(
                    {
                        "workflow": path,
                        "job": job_id,
                        "step": s.get("name", f"step-{i+1}"),
                        "runsOn": job.get("runs-on"),
                        "matrix": job.get("strategy", {}).get("matrix"),
                        "jobIf": job.get("if"),
                        "stepIf": s.get("if"),
                        "explicitIgnored": "--ignored" in cmd,
                        "stepRunSha256": sha256(cmd.encode()).hexdigest(),
                    }
                )
    return steps
