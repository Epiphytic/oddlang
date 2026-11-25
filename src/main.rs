use oddlang::model::load_model;
use std::{env, process::ExitCode};
fn run(args: &[String]) -> Result<(), String> {
    match args {
        [group, cmd, project, lock, cache, overlay, output]
            if group == "http" && cmd == "generate" =>
        {
            let contract = oddlang::http::load(project.as_ref(), lock.as_ref(), cache.as_ref())?;
            let overlay = oddlang::http::load_overlay(overlay.as_ref())?;
            for binding in overlay.bindings {
                match binding.binding.as_str() {
                    "fetch" => oddlang::http::emit_typescript(&contract, output.as_ref())?,
                    "axum" => {
                        let pin = contract
                            .dependencies
                            .iter()
                            .find(|pin| pin.identity == "urn:oddlang:contract:ledger")
                            .ok_or("missing ledger dependency")?;
                        let ledger = oddlang::contracts::resolve_pin(pin, cache.as_ref())?;
                        oddlang::http::emit_rust(&contract, output.as_ref())?;
                        std::fs::write(
                            std::path::Path::new(output).join("ledger.rs"),
                            oddlang::targets::rust::generate(&ledger.model),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    unknown => return Err(format!("unsupported binding {unknown}")),
                }
            }
            Ok(())
        }
        [group, cmd, input, lock, cache, output] if group == "http" && cmd == "publish" => {
            let c = oddlang::http::load_source(input.as_ref(), lock.as_ref(), cache.as_ref())?;
            oddlang::http::publish(&c, output.as_ref())
        }
        [group, cmd, bundle, pin, claim, evidence]
            if group == "http" && cmd == "verify-evidence" =>
        {
            use oddlang::http::{read_json, verify_bundle, verify_evidence};
            let bundle = verify_bundle(bundle.as_ref(), pin.as_ref())?;
            verify_evidence(
                &bundle,
                &read_json(claim.as_ref())?,
                &read_json(evidence.as_ref())?,
            )
        }
        [group, cmd, input, lock, cache] if group == "http" => {
            let load = match cmd.as_str() {
                "check-source" => oddlang::http::load_source,
                "check" => oddlang::http::load,
                _ => return Err(format!("unknown HTTP command {cmd}")),
            };
            load(input.as_ref(), lock.as_ref(), cache.as_ref())?;
            Ok(())
        }
        [cmd, dir] if cmd == "check" => {
            load_model(dir)?;
            Ok(())
        }
        [cmd, dir, iri] if cmd == "impact" => {
            for result in oddlang::impact::impact(&load_model(dir)?, iri)? {
                println!("{result}")
            }
            Ok(())
        }
        [cmd, dir, out] if cmd == "build" => {
            oddlang::targets::build(&load_model(dir)?, std::path::Path::new(out))
                .map_err(|e| e.to_string())
        }
        _ => Err("usage: oddlang check PROJECT | impact PROJECT IRI | build PROJECT OUTPUT".into()),
    }
}
fn main() -> ExitCode {
    match run(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
