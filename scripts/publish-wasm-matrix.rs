//! Publish the Phase 1 compatibility fixtures and independent host-binding probe.
use oddlang::wasm::bundle;
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};

fn publish(out: &Path) -> Result<(), Box<dyn Error>> {
    let contracts = Path::new("contracts/wasm");
    for (base, variant) in [
        ("echo", "label"),
        ("echo", "rename"),
        ("echo", "tags"),
        ("echo", "import"),
        ("ledger", "probe"),
    ] {
        let original = contracts.join(base);
        let temp = tempfile::tempdir()?;
        let source = temp.path();
        let mut model = fs::read_to_string(original.join("model.ttl"))?;
        let mut corpus: Value = serde_json::from_slice(&fs::read(original.join("corpus.json"))?)?;
        match variant {
            "label" => model.push_str("\ne:echo w:label \"Internal description\" .\n"),
            "rename" => {
                model = model.replace("w:name \"echo\"", "w:name \"mirror\"");
                for call in corpus[0]["calls"]
                    .as_array_mut()
                    .ok_or("expected echo calls")?
                {
                    call["function"] = json!("mirror");
                }
            }
            "tags" => {
                for (name, old, new) in [("empty", 0, 1), ("number", 1, 0)] {
                    model = model.replace(
                        &format!("w:name \"{name}\" ; w:ordinal {old}"),
                        &format!("w:name \"{name}\" ; w:ordinal {new}"),
                    );
                }
            }
            "import" => {
                model.push_str("\ne:world w:import e:seed .\ne:seed a w:Function ; w:name \"seed\" ; w:result \"s64\" .\n");
                corpus[0]["covers"]
                    .as_array_mut()
                    .ok_or("expected echo coverage")?
                    .push(json!({
                        "declaration": "urn:oddlang:echo:seed", "role": "wasm.importer"
                    }));
                corpus[0]["imports"] = json!([{"function":"seed", "arguments":[], "returns":"1"}]);
            }
            "probe" => {
                corpus =
                    serde_json::from_slice(&fs::read("examples/wasm/claims/probe-corpus.json")?)?
            }
            _ => unreachable!(),
        }
        fs::write(source.join("model.ttl"), model)?;
        fs::write(source.join("corpus.json"), serde_json::to_vec(&corpus)?)?;
        fs::copy(
            original.join("dependencies.json"),
            source.join("dependencies.json"),
        )?;
        bundle::publish(
            source,
            &contracts.join("empty.lock.json"),
            Path::new("contracts/cache"),
            &out.join(format!("{base}-{variant}")),
        )?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let out = args.next().ok_or("usage: publish-wasm-matrix OUTPUT")?;
    if args.next().is_some() {
        return Err("usage: publish-wasm-matrix OUTPUT".into());
    }
    publish(out.as_ref())
}
