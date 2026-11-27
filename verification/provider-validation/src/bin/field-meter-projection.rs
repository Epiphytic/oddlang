//! Compare the generated field-meter projection with independent request and reply obligations.
use provider_validation::{Result, cli, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

fn project(cli: &Path, root: &Path, graph: &Value, output: &str, error: Option<&str>) -> Result {
    write(&root.join("graph.json"), graph)?;
    invoke(
        cli,
        root,
        &[
            "build",
            "local-command-client",
            "rust",
            "graph.json",
            "binding.json",
        ],
        output,
        error,
    )
}

fn catalog(root: &Path, name: &str, digest_field: &str) -> Result<Value> {
    let path = root.join(format!("{name}-obligations.json"));
    let lock = read(&root.join("projection.lock"))?;
    if lock[digest_field] != sha(&path)? {
        return Err(format!("incorrect {name} catalog digest").into());
    }
    read(&path)
}

fn check_request(root: &Path) -> Result {
    let catalog = catalog(root, "request", "requestObligationsSha256")?;
    let rows = catalog["obligations"]
        .as_array()
        .ok_or("missing obligations")?;
    let (header, fields) = rows.split_first().ok_or("missing request envelope")?;
    let keys = ["verb", "sensor", "label", "waiting", "probes", "comment"];
    if catalog["qualification"] != "independent-witness-required"
        || header["declaration"] != "urn:field:request"
        || header["role"] != "request-envelope"
        || header["maximumFrameBytes"] != 128
        || header["objectOrder"] != "declarationOrder"
        || header["argumentSource"] != "explicitCommandLine"
        || header["stringEscaping"]["otherControls"] != "lowercase-u00xx"
        || header["stringEscaping"]["controlRange"] != "U+0000-U+001F"
        || fields.len() != keys.len()
    {
        return Err("incorrect field-meter request envelope".into());
    }
    for (index, (field, key)) in fields.iter().zip(keys).enumerate() {
        if field["declaration"] != format!("urn:field:request:{key}")
            || field["role"] != "request-field"
            || field["position"] != index
            || field["key"] != key
        {
            return Err(format!("incorrect request field {key}").into());
        }
    }
    if fields[1]["argument"] != json!({"id":"sensor","kind":"unsigned64"})
        || fields[1]["representation"]
            != json!({"type":"integer","minimumDecimal":"0","maximumDecimal":"18446744073709551615","absent":"reject"})
        || fields[5].get("value") != Some(&Value::Null)
    {
        return Err("incorrect sensor range or comment literal".into());
    }
    Ok(())
}

type Mutation = (fn(&mut Value), &'static str);
fn reject(cli: &Path, root: &Path, graph: &Value, cases: &[Mutation]) -> Result {
    for (mutate, diagnostic) in cases {
        let mut changed = graph.clone();
        mutate(&mut changed);
        project(cli, root, &changed, "invalid", Some(diagnostic))?;
    }
    Ok(())
}

fn main() -> Result {
    let cli = cli()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../local-command-client");
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    write(
        &root.join("binding.json"),
        &json!({"format":"oddlang-local-command-client-rust-v1","entry":"measure"}),
    )?;
    let base = read(&fixture.join("graph.json"))?;
    let mut request = base.clone();
    request["request_id"] = json!("urn:field:request");
    request["request"] = read(&fixture.join("typed-request.json"))?;
    project(&cli, root, &request, "request", None)?;
    check_request(&root.join("request"))?;
    reject(
        &cli,
        root,
        &request,
        &[
            (
                |g| g["transport"]["maximum_frame_bytes"] = json!(1),
                "minimum request exceeds frame limit",
            ),
            (
                |g| {
                    g.as_object_mut().unwrap().remove("request_id");
                },
                "missing request declaration identity",
            ),
            (
                |g| g["request"][1]["id"] = json!("urn:field:request"),
                "duplicate request declaration identity",
            ),
            (
                |g| g["request"][5]["value"] = json!({"samples":[0.25]}),
                "identified request literals require signed or unsigned integer JSON numbers",
            ),
            (
                |g| g["request"][1]["value"] = Value::Null,
                "request field needs exactly one literal value or argument",
            ),
            (
                |g| {
                    g["request"][1].as_object_mut().unwrap().remove("argument");
                },
                "request field needs exactly one literal value or argument",
            ),
        ],
    )?;
    let minimum =
        br#"{"verb":"read","sensor":0,"label":"","waiting":true,"probes":[],"comment":null}"#;
    request["transport"]["maximum_frame_bytes"] = json!(minimum.len());
    project(&cli, root, &request, "minimum", None)?;
    let mut reply = base.clone();
    reply["response"] = read(&fixture.join("typed-response.json"))?;
    reply["transport"]["maximum_frame_bytes"] = json!(512);
    project(&cli, root, &reply, "reply", None)?;
    let reply_catalog = catalog(&root.join("reply"), "reply", "replyObligationsSha256")?;
    let rows = reply_catalog["obligations"]
        .as_array()
        .ok_or("missing reply obligations")?;
    let identities = rows
        .iter()
        .map(|r| r["declaration"].as_str().ok_or("missing reply identity"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    if rows.len() != 12 || identities.len() != 12 {
        return Err("expected twelve distinct reply declarations".into());
    }
    reject(
        &cli,
        root,
        &reply,
        &[
            (
                |g| {
                    g["response"]
                        .as_object_mut()
                        .unwrap()
                        .remove("ordered_sequence_minimum");
                    g["response"]["fields"][2]["default"] = json!(true);
                },
                "reply field default requires ordered_sequence_minimum",
            ),
            (
                |g| g["response"]["ordered_sequence_minimum"] = json!(1),
                "invalid nullable suffix",
            ),
            (
                |g| g["response"]["fields"][0]["kind"]["id"] = json!("urn:field:missing"),
                "unknown reply type",
            ),
            (
                |g| g["response"]["fields"][0]["id"] = json!("urn:field:reading"),
                "duplicate reply declaration",
            ),
        ],
    )?;
    reject(
        &cli,
        root,
        &base,
        &[(
            |g| g["response"]["fields"][0]["kind"] = json!("u8"),
            "typed reply needs a root declaration identity",
        )],
    )?;
    println!("verified request and reply catalogs, exact minimum frame, and 11 rejected graphs");
    Ok(())
}
