//! Independent public API and source-backed value fixtures.
use super::tests::{checked_output, crate_files, fixtures, host};
use super::*;
use std::os::unix::fs::PermissionsExt;

pub fn api(cli: &Path, root: &Path) -> Result {
    let source = fs::read_to_string(fixtures().join("api.rs"))?;
    let witness = |s: &str, i: usize, error: Option<&str>| -> Result<Vec<u8>> {
        let lib = root.join("lib.rs");
        let dir = root.join(i.to_string());
        fs::write(&lib, s)?;
        checked(
            cmd("rustdoc")
                .env("RUSTC_BOOTSTRAP", "sensor")
                .args([
                    "-Z",
                    "unstable-options",
                    "--output-format",
                    "json",
                    "--edition=2021",
                    "--crate-name",
                    "sensor",
                ])
                .arg(&lib)
                .arg("-o")
                .arg(&dir),
        )?;
        Ok(checked_output(
            cmd(cli)
                .args(["inspect", "native-value-api"])
                .arg(dir.join("sensor.json")),
            error,
        )?
        .stdout)
    };
    let raw = witness(&source, 0, None)?;
    let facts: Value = serde_json::from_slice(&raw)?;
    assert_eq!(
        object(&facts["types"])?.keys().collect::<Vec<_>>(),
        ["Phase", "Sample"]
    );
    assert_eq!(
        witness(
            &source
                .replace("pressure", "bytes")
                .replace("detail", "implementation"),
            1,
            None
        )?,
        raw
    );
    for (i, (a, b)) in [
        ("Idle, Busy", "Busy, Idle"),
        ("&[u8]", "&mut [u8]"),
        ("Clone, Copy", "Clone"),
        ("const LIMIT: u16", "const LIMIT: u32"),
        ("pub fn read", "fn read"),
        (
            "-> u16 { self.0 + pressure.len() as u16 }",
            "-> u32 { u32::from(self.0) + pressure.len() as u32 }",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let next: Value = serde_json::from_slice(&witness(&source.replace(a, b), i + 2, None)?)?;
        assert_ne!(next, facts);
    }
    witness(
        &source.replace("pub struct Sample(u16)", "pub struct Sample(pub u16)"),
        9,
        Some("public tuple field"),
    )?;
    println!(
        "Two equivalent facades matched, six public changes distinguished, unsupported shape rejected"
    );
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(checked(cmd("git").current_dir(root).args(args))?)?
            .trim()
            .to_string(),
    )
}
fn commit(root: &Path, file: &str) -> Result<String> {
    git(root, &["add", file])?;
    git(
        root,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    git(root, &["rev-parse", "HEAD"])
}
pub fn shared(cli: &Path, work: &Path) -> Result {
    let root = work.join("sensor");
    fs::create_dir(&root)?;
    git(&root, &["init", "-q"])?;
    let mut selection = json!({"moduleFile":"lib.rs","domain":"urn:t:values","constants":{"CAP":"urn:t:cap","SOFT":"urn:t:soft"},
        "errors":{"Alarm":{"id":"urn:t:alarm","variants":{"Reading":"urn:t:reading","Missing":"urn:t:missing"}}},"results":{"ReadingResult":"urn:t:result"}});
    let decl = work.join("selection.json");
    let run = |out: &Path, error: Option<&str>| {
        checked_output(
            cmd(cli)
                .args(["recover", "shared-declarations"])
                .arg(&root)
                .arg(&decl)
                .arg(out),
            error,
        )
    };
    let cases: Vec<(String, bool)> =
        serde_json::from_slice(&fs::read(fixtures().join("shared-cases.json"))?)?;
    for (i, (source, succeeds)) in cases.into_iter().enumerate() {
        fs::write(root.join("lib.rs"), source)?;
        let revision = commit(&root, "lib.rs")?;
        fs::write(root.join("lib.rs"), "working tree deliberately differs")?;
        selection["sourceRevision"] = json!(revision);
        write(&decl, &selection)?;
        let out = work.join(i.to_string());
        run(&out, (!succeeds).then_some(""))?;
        assert_eq!(out.exists(), succeeds);
        if succeeds {
            let g = read(&out.join("graph.json"))?;
            let nums = if i == 0 { [3, 17] } else { [4, 19] };
            assert_eq!(g["constants"][0]["value"], json!({"multiply":nums}));
            assert_eq!(g["constants"][1]["value"], json!({"ref":"urn:t:cap"}));
            let variants = &g["errors"][0]["variants"];
            assert_eq!(
                variants[0]["message"],
                json!([{"text":"sensor {limit}: "},{"payload":true}])
            );
            assert_eq!(
                variants[1]["message"],
                json!([{"text":if i==0{"no signal"}else{"disconnected"}}])
            );
            assert_eq!(
                read(&out.join("observation.json"))?["sourceRevision"],
                revision
            );
            run(&out, Some(""))?;
        }
    }
    for (key, val) in [
        ("moduleFile", "../lib.rs".to_owned()),
        ("sourceRevision", git(&root, &["rev-parse", "HEAD:lib.rs"])?),
    ] {
        let mut s = selection.clone();
        s[key] = json!(val);
        write(&decl, &s)?;
        run(&work.join(format!("bad-{key}")), Some(""))?;
    }
    println!("Verified pinned revisions, symbolic references, typed messages and rejections");
    Ok(())
}
pub fn value(cli: &Path, base: &Path) -> Result {
    let root = base.join("crate");
    crate_files(
        &root,
        "[package]\nname='sensor'\nversion='0.1.0'\nedition='2024'",
        "",
    )?;
    fs::write(root.join(".gitignore"), "/.cargo/\n")?;
    git(&root, &["init", "-q"])?;
    let template = fs::read_to_string(fixtures().join("value-domain.rs.in"))?;
    for (maximum, hole) in [(11, false), (13, false), (13, true)] {
        fs::write(
            root.join("src/lib.rs"),
            template
                .replace("MAXIMUM", &maximum.to_string())
                .replace("HOLE", if hole { "|| v == 9" } else { "" }),
        )?;
        let revision = commit(&root, ".")?;
        let selection = json!({"package":"sensor","bits":8,"rangeError":"urn:s:outside","sourceRevision":revision,"cargoLockSha256":sha(&root.join("Cargo.lock"))?,
            "compilerCommit":PIN,"api":{"domain":"urn:s:pressure","typeName":"Sensor","constructor":"sample","accessor":"reading","maximumConstant":"CEILING","errorType":"Alarm",
            "variants":{"urn:s:outside":"Outside","urn:s:broken":"Broken"},"variantOrder":["urn:s:outside","urn:s:broken"]}});
        let select = base.join("selection.json");
        write(&select, &selection)?;
        let exe = |sel: &Path, out: &Path| {
            let mut c = cmd(cli);
            c.args(["recover", "value-domain"])
                .arg(root.join("Cargo.toml"))
                .arg(sel)
                .arg(out);
            c
        };
        let out = base.join(&revision);
        checked_output(&mut exe(&select, &out), hole.then_some(""))?;
        if hole {
            assert!(!out.exists());
            continue;
        }
        assert!(
            fs::read_to_string(out.join("domain.ttl"))?.contains(&format!(
                "c:minimum 3 ; c:maximum {maximum} ; c:defaultValue 5"
            ))
        );
        assert_eq!(
            read(&out.join("observation.json"))?["numericCoverage"],
            "exhaustive"
        );
        if maximum == 13 {
            let mut wrong = selection.clone();
            wrong["api"]["variantOrder"] = json!(["urn:s:broken", "urn:s:outside"]);
            let p = base.join("wrong.json");
            write(&p, &wrong)?;
            let dest = base.join("unexpected-order");
            checked_output(&mut exe(&p, &dest), Some("unexpected enum ordinal"))?;
            assert!(!dest.exists());
            fs::write(
                root.join(".git/info/attributes"),
                "src/lib.rs export-ignore\n",
            )?;
            for dir in [
                root.join(".cargo"),
                base.join(".cargo"),
                base.join("user-cargo"),
            ] {
                fs::create_dir(&dir)?;
                fs::write(
                    dir.join("config.toml"),
                    "[build]\nrustflags = ['--cfg','oddlang_injected']\n",
                )?;
            }
            let marker = base.join("wrapper-ran");
            let wrapper = base.join("compiler-wrapper");
            fs::write(
                &wrapper,
                format!("#!/bin/sh\ntouch \"{}\"\nexec \"$@\"\n", marker.display()),
            )?;
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))?;
            let isolated = base.join("isolated");
            checked(
                exe(&select, &isolated)
                    .env("CARGO_HOME", base.join("user-cargo"))
                    .env("CARGO_BUILD_RUSTC", &wrapper)
                    .env("CARGO_BUILD_RUSTC_WRAPPER", &wrapper)
                    .env("RUSTC_WORKSPACE_WRAPPER", &wrapper)
                    .env("CARGO_ENCODED_RUSTFLAGS", "--cfg\u{1f}oddlang_injected"),
            )?;
            assert!(!marker.exists());
            for f in ["domain.ttl", "observation.json"] {
                assert_eq!(fs::read(isolated.join(f))?, fs::read(out.join(f))?);
            }
            let o = read(&isolated.join("observation.json"))?;
            let hash = string(&o["probeSha256"])?;
            assert!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
        }
    }
    println!(
        "Two revisions recovered; hidden config and wrappers isolated; noninterval source and wrong enum order rejected"
    );
    Ok(())
}
pub fn inherent(exe: &Path, root: &Path) -> Result {
    let source = root.join("src/lib.rs");
    crate_files(
        root,
        "[package]\nname='optic'\nversion='0.1.0'\nedition='2024'\n",
        &fs::read_to_string(fixtures().join("inherent.rs"))?,
    )?;
    let file = root.join("report.json");
    checked(
        cmd(exe)
            .arg(root)
            .args([
                "optic",
                "lib",
                "optic",
                &host()?,
                "--public-inherent",
                "--out",
            ])
            .arg(&file),
    )?;
    let report = read(&file)?;
    let inv = &report["rustdocPublicInherent"];
    let owner = json!(["optic", "sensor", "Meter"]);
    assert_eq!(
        inv["items"],
        json!([{"owner":owner,"kind":"assoc_const","name":"LIMIT"},{"owner":owner,"kind":"function","name":"hidden_from_docs"},{"owner":owner,"kind":"function","name":"read"}])
    );
    assert_eq!(
        inv["excludedIndexedLocalInherentImpls"]["generic-or-specialized-receiver"],
        1
    );
    checked(
        cmd("rustc")
            .current_dir(root)
            .args(["--crate-type=rlib", "--crate-name", "optic"])
            .arg(source)
            .args(["-o", "liboptic.rlib"]),
    )?;
    for (body, error) in [
        (
            "let _=Meter::LIMIT;Meter.read();Meter::hidden_from_docs();Meter::compiler_only();Batch(1u8).specific();",
            None,
        ),
        ("Meter::hidden();", Some("E0624")),
        ("Batch(1u16).specific();", Some("E0599")),
    ] {
        fs::write(
            root.join("caller.rs"),
            format!("use optic::sensor::{{Batch,Meter}};fn main(){{{body}}}"),
        )?;
        checked_output(
            cmd("rustc").current_dir(root).args([
                "--edition=2024",
                "--emit=metadata",
                "--extern",
                "optic=liboptic.rlib",
                "caller.rs",
                "-o",
                "caller.rmeta",
            ]),
            error,
        )?;
    }
    Ok(())
}
