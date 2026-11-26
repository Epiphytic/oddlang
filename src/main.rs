use oddlang::model::load_model;
use std::{env, process::ExitCode};
fn mapping_has_profile(path: &str, profile: &str) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
        .and_then(|table| table.get("profile")?.as_str().map(str::to_owned))
        .as_deref()
        == Some(profile)
}

fn import_has_profile(path: &str, profile: &str) -> bool {
    let lock = std::path::Path::new(path).join("import.lock");
    std::fs::read(lock)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value["profile"] == profile)
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [group, kind, source, selection, overlay, variant, output]
            if group == "recover" && kind == "tagged-contract" =>
        {
            oddlang::tagged_wire::recover(
                source.as_ref(), selection.as_ref(), overlay.as_ref(), variant, output.as_ref(),
            )
        }
        [command, kind, target, graph, output, flag, pin]
            if command == "build" && kind == "tagged-wire" && flag == "--graph-sha256" =>
        {
            oddlang::tagged_wire::build(graph.as_ref(), pin, target, output.as_ref())
        }
        [group, kind, root, selection]
            if group == "recover" && kind == "tagged-variant" =>
        {
            let bytes = std::fs::read(selection).map_err(|err| err.to_string())?;
            let selection = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
            let candidate = oddlang::rust_recovery::tagged::recover(root.as_ref(), &selection)?;
            println!("{}", serde_json::to_string_pretty(&candidate).map_err(|err| err.to_string())?);
            Ok(())
        }
        [group, command, graph] if group == "tagged-wire" && command == "digest" => {
            println!("{}", oddlang::tagged_wire::load(graph.as_ref())?.sha256());
            Ok(())
        }
        [group, kind, manifest, profile_flag, profile, out_flag, output]
            if group == "recover" && kind == "rust" && profile_flag == "--profile" && out_flag == "--out" =>
        {
            oddlang::rust_recovery::recover(manifest.as_ref(), profile.as_ref(), output.as_ref())
        }
        [group, kind, input, output, flag, package]
            if group == "recover" && kind == "validate-inventory" && flag == "--package" =>
        {
            oddlang::rust_recovery::validate_scoped_inventory(input.as_ref(), output.as_ref(), Some(package))
        }
        [group, kind, input, output] if group == "recover" && kind == "validate-inventory" => {
            oddlang::rust_recovery::validate_inventory(input.as_ref(), output.as_ref())
        }
        [command, kind, manifest, package, output] if command == "import" && kind == "rust-tree" => {
            oddlang::rust_roundtrip::capture(manifest.as_ref(), package, output.as_ref())
        }
        [command, kind, bundle, output, flag, pin]
            if command == "build" && kind == "rust-tree" && flag == "--bundle-sha256" =>
        {
            oddlang::rust_roundtrip::build(bundle.as_ref(), output.as_ref(), pin)
        }
        [group, cmd, manifest, mapping, out]
            if group == "import"
                && cmd == "rust"
                && mapping_has_profile(mapping, "rust-facade-v1") =>
        {
            oddlang::facade::import(manifest.as_ref(), mapping.as_ref(), out.as_ref())
        }
        [group, cmd, dir, flag, pin]
            if group == "import"
                && cmd == "verify"
                && flag == "--import-sha256"
                && import_has_profile(dir, "rust-facade-v1") =>
        {
            oddlang::facade::verify_pinned(dir.as_ref(), pin)
        }
        [cmd, kind, dir, out, flag, pin]
            if cmd == "build"
                && kind == "module"
                && flag == "--import-sha256"
                && import_has_profile(dir, "rust-facade-v1") =>
        {
            oddlang::facade::build_pinned(dir.as_ref(), pin, out.as_ref())
        }
        [group, cmd, manifest, mapping, dir]
            if group == "import"
                && cmd == "replay"
                && mapping_has_profile(mapping, "rust-facade-v1") =>
        {
            let fresh = tempfile::tempdir().map_err(|e| e.to_string())?;
            let output = fresh.path().join("import");
            oddlang::facade::import(manifest.as_ref(), mapping.as_ref(), &output)?;
            oddlang::framing_projection::compare_directories(&output, dir.as_ref())
        }

        [cmd, manifest, store, out] if cmd == "assemble" => {
            oddlang::assemble::assemble(manifest.as_ref(), store.as_ref(), out.as_ref())
        }
        [cmd, kind, import, out, flag, pin]
            if cmd == "build" && kind == "module" && flag == "--import-sha256" =>
        {
            oddlang::framing_projection::build_pinned(import.as_ref(), pin, out.as_ref())
        }
        [group, cmd, dir, flag, pin]
            if group == "import" && cmd == "verify" && flag == "--import-sha256" =>
        {
            oddlang::framing_import::verify_pinned(dir.as_ref(), pin)
        }
        [cmd, kind, import, out] if cmd == "build" && kind == "module" => {
            oddlang::framing_projection::build(import.as_ref(), out.as_ref())
        }
        [group, cmd, manifest, mapping, out]
            if group == "import" && cmd == "rust" && mapping_has_profile(mapping, "framing-v1") =>
        {
            oddlang::framing_import::import(manifest.as_ref(), mapping.as_ref(), out.as_ref())
        }
        [group, cmd, manifest, mapping, out] if group == "import" && cmd == "rust" => {
            oddlang::import_rust::import(manifest.as_ref(), mapping.as_ref(), out.as_ref(), None)
        }
        [
            group,
            cmd,
            manifest,
            mapping,
            out,
            flag,
            receipt,
            pin_flag,
            pin,
        ] if group == "import"
            && cmd == "rust"
            && flag == "--accepted-cleanup"
            && pin_flag == "--baseline-pin" =>
        {
            oddlang::import_rust::import(
                manifest.as_ref(),
                mapping.as_ref(),
                out.as_ref(),
                Some((receipt.as_ref(), pin.as_ref())),
            )
        }
        [group, cmd, dir, output]
            if group == "import"
                && cmd == "verify-build"
                && import_has_profile(dir, "framing-v1") =>
        {
            oddlang::framing_projection::verify_build(dir.as_ref(), output.as_ref())
        }
        [group, cmd, dir]
            if group == "import" && cmd == "verify" && import_has_profile(dir, "framing-v1") =>
        {
            oddlang::framing_import::verify(dir.as_ref())
        }
        [group, cmd, manifest, mapping, dir]
            if group == "import"
                && cmd == "replay"
                && mapping_has_profile(mapping, "framing-v1") =>
        {
            let fresh = tempfile::tempdir().map_err(|e| e.to_string())?;
            let output = fresh.path().join("import");
            oddlang::framing_import::import(manifest.as_ref(), mapping.as_ref(), &output)?;
            oddlang::framing_projection::compare_directories(&output, dir.as_ref())
        }
        [group, cmd, dir, output] if group == "import" && cmd == "verify-build" => {
            oddlang::import_rust::verify_build(dir.as_ref(), output.as_ref())
        }
        [group, cmd, dir] if group == "import" && cmd == "verify" => {
            oddlang::import_rust::verify(dir.as_ref())
        }
        [group, cmd, manifest, mapping, dir] if group == "import" && cmd == "replay" => {
            oddlang::import_rust::replay(manifest.as_ref(), mapping.as_ref(), dir.as_ref(), None)
        }
        [
            group,
            cmd,
            manifest,
            mapping,
            dir,
            flag,
            receipt,
            pin_flag,
            pin,
        ] if group == "import"
            && cmd == "replay"
            && flag == "--accepted-cleanup"
            && pin_flag == "--baseline-pin" =>
        {
            oddlang::import_rust::replay(
                manifest.as_ref(),
                mapping.as_ref(),
                dir.as_ref(),
                Some((receipt.as_ref(), pin.as_ref())),
            )
        }
        [group, rest @ ..] if group == "wasm" => oddlang::wasm::command(rest),
        [group, cmd, project, lock, cache, overlay, output]
            if group == "http" && cmd == "generate" =>
        {
            let contract = oddlang::http::load(project.as_ref(), lock.as_ref(), cache.as_ref())?;
            let overlay = oddlang::http::load_overlay(overlay.as_ref())?;
            for binding in overlay.bindings {
                match binding.binding.as_str() {
                    "serde" => oddlang::http::emit_codec_rust(&contract, output.as_ref())?,
                    "json" => oddlang::http::emit_codec_typescript(&contract, output.as_ref())?,
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
        [group, cmd, input, lock, cache, output, option, predecessor]
            if group == "http" && cmd == "publish" && option == "--predecessor" =>
        {
            let c = oddlang::http::load_source(input.as_ref(), lock.as_ref(), cache.as_ref())?;
            oddlang::http::publish_successor(
                &c,
                predecessor.as_ref(),
                cache.as_ref(),
                output.as_ref(),
            )
        }
        [group, cmd, project, old, new, cache] if group == "http" && cmd == "compatibility" => {
            let a = oddlang::http::load(project.as_ref(), old.as_ref(), cache.as_ref())?;
            let b = oddlang::http::load(project.as_ref(), new.as_ref(), cache.as_ref())?;
            let report = oddlang::http::compatibility(&a, &b);
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if report.old_client_new_provider != oddlang::http::Verdict::Compatible
                || report.new_client_old_provider != oddlang::http::Verdict::Compatible
            {
                return Err(
                    "automatic promotion requires compatible peers in both directions".into(),
                );
            }
            Ok(())
        }
        [group, cmd, project, pin, cache, iris @ ..]
            if group == "http" && cmd == "affected" && !iris.is_empty() =>
        {
            let c = oddlang::http::load(project.as_ref(), pin.as_ref(), cache.as_ref())?;
            let ids = iris
                .iter()
                .map(|s| oddlang::http::Iri::new(s.as_str()))
                .collect::<Result<Vec<_>, _>>()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&oddlang::http::affected(&c, &ids)).unwrap()
            );
            Ok(())
        }
        [group, cmd, root, lock, bundle] if group == "http" && cmd == "verify-target-lock" => {
            oddlang::http::overlay::verify_target_lock(
                root.as_ref(),
                lock.as_ref(),
                bundle.as_ref(),
            )
            .map(|_| ())
        }
        [group, cmd, project, lock, cache] if group == "http" && cmd == "check-workspace" => {
            oddlang::http::load_workspace(project.as_ref(), lock.as_ref(), cache.as_ref())
                .map(|_| ())
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
            if std::path::Path::new(dir)
                .join("import.lock")
                .symlink_metadata()
                .is_ok()
            {
                oddlang::import_rust::verify(dir.as_ref())?;
            }
            oddlang::targets::build(&load_model(dir)?, std::path::Path::new(out))
                .map_err(|e| e.to_string())
        }
        _ => Err("usage: oddlang check PROJECT | impact PROJECT IRI | build PROJECT OUTPUT | recover rust MANIFEST --profile PROFILE --out OUTPUT | recover tagged-contract SOURCE SELECTION OVERLAY VARIANT OUTPUT | tagged-wire digest GRAPH | build tagged-wire TARGET GRAPH OUTPUT --graph-sha256 PIN | import rust-tree MANIFEST PACKAGE OUTPUT | build rust-tree BUNDLE OUTPUT --bundle-sha256 PIN".into()),
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
