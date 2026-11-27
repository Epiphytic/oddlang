use super::*;
use serde_json::json;
pub(super) fn verify(s: &Suite, cases: &[Case], base: &str) -> Result {
    let relation = "urn:sample:agrees";
    let template = s.fixtures.join("template.json");
    let overlay = s.fixtures.join("rust.json");
    let types = s.project.join("src/types.rs");
    let standard = s.fixtures.join("types.rs");
    let custom = s.fixtures.join("custom-types.rs");
    fs::copy(&standard, s.work.join("types.rs"))?;
    let prefix = base
        .split_once("impl Invoice")
        .ok_or("missing inherent impl")?
        .0;
    for case in cases {
        let source = case.revision(base)?;
        let text = if case.name == "base" {
            source.replace('\n', "\r\n")
        } else {
            source.clone()
        };
        fs::write(&s.source, text)?;
        fs::copy(
            if case.name == "custom" {
                &custom
            } else {
                &standard
            },
            &types,
        )?;
        let observed = s.observe(&s.work.join("compiler.json"))?;
        assert_eq!(observed["format"], "oddlang-rust-field-conjunction-v1");
        s.select(json!({"format":"oddlang-field-conjunction-selection-v1","relation":relation}))?;
        let out = s.work.join(format!("{}.json", case.name));
        s.recover(
            "field-conjunction",
            &template,
            &overlay,
            &out,
            case.unresolved.as_deref(),
        )?;
        let Some(expected) = &case.expected else {
            continue;
        };
        if case.name == "base" {
            fs::copy(&custom, &types)?;
            s.recover(
                "field-conjunction",
                &template,
                &overlay,
                &s.work.join("stale.json"),
                Some("inventory changed: src/types.rs"),
            )?;
            fs::copy(&standard, &types)?;
        }
        let repeat = s.work.join(format!("{}-repeat.json", case.name));
        s.recover("field-conjunction", &template, &overlay, &repeat, None)?;
        assert_eq!(fs::read(&repeat)?, fs::read(&out)?);
        let code = s.build("field-relation", &out, &overlay, &s.work.join(&case.name))?;
        s.native(&source, expected)?;
        s.native(&format!("{prefix}{code}"), expected)?;
    }
    let original = read(&s.work.join("base.json"))?;
    let changed = read(&s.work.join("mutated.json"))?;
    let [left, right] =
        [&original, &changed].map(|g| g["relations"][relation]["steps"].as_array().unwrap());
    assert_eq!(left[1..], right[1..]);
    for (steps, name) in [(left, ":serial"), (right, ":backup")] {
        assert!(steps[0]["left"]["field"].as_str().unwrap().ends_with(name));
    }
    Ok(())
}
