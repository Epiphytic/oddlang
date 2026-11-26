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
        [cmd, kind, observation, selection, output] if cmd == "recover" && kind == "fixed-bytes" => {
            oddlang::checked_values::recover_fixed_bytes(observation.as_ref(), selection.as_ref(), output.as_ref())
        }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "checked-values" && target == "rust" =>
        {
            oddlang::checked_values::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, source, selection, output] if cmd == "recover" && kind == "finite-error-domain" => {
            oddlang::finite_error_domain::recover(source.as_ref(), selection.as_ref(), output.as_ref())
        }
        [cmd, kind, target, graph, overlay, output] if cmd == "build" && kind == "finite-error-domain" && target == "rust" => {
            oddlang::finite_error_domain::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, root, selection, out] if cmd == "recover" && kind == "build-report" => {
            oddlang::build_report::recover(root.as_ref(), selection.as_ref(), out.as_ref())
        }
        [group, kind, target, graph, binding, out]
            if group == "build" && kind == "local-status-owner" && target == "rust" =>
        {
            oddlang::local_status_owner::build(graph.as_ref(), binding.as_ref(), out.as_ref())
        }
        [group, kind, target, graph, binding, out]
           if group == "build" && kind == "file-profile" && target == "rust" =>
       {
           oddlang::file_profile::build(graph.as_ref(), binding.as_ref(), out.as_ref())
        }
        [group, kind, target, graph, binding, out]
            if group == "build" && kind == "local-command-client" && target == "rust" =>
        {
            oddlang::local_command_client::build(graph.as_ref(), binding.as_ref(), out.as_ref())
        }
        [cmd,kind,target,manifest,out] if cmd=="build" && kind=="native-workspace" && target=="rust" =>
            oddlang::native_workspace::build(manifest.as_ref(),out.as_ref()),

        [cmd, kind, target, shape, shape_binding, protocol, protocol_binding, output]
            if cmd == "build" && kind == "wire-protocol" && target == "rust" => {
                oddlang::wire_schema::protocol::build(shape.as_ref(), shape_binding.as_ref(), protocol.as_ref(), protocol_binding.as_ref(), output.as_ref())
            }
        [cmd, kind, source, selection, output] if cmd == "recover" && kind == "record-writer" => {
            oddlang::wire_schema::record_writer::recover(source.as_ref(), selection.as_ref(), output.as_ref())
        }
        [cmd, kind, source, selection, output]
            if cmd == "recover" && kind == "wire-schema" => {
                oddlang::wire_schema::recover::from_source(source.as_ref(), selection.as_ref(), output.as_ref())
            }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "field-relation" && target == "rust" =>
        {
            oddlang::field_relation::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "binary-frame" && target == "rust" =>
        {
            oddlang::binary_frame::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "binary-record" && target == "rust" =>
        {
            oddlang::binary_record::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "wire-schema" && target == "rust" => {
                oddlang::wire_schema::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
            }
        [cmd, kind, target, graph, overlay, output]
            if cmd == "build" && kind == "integrity-block" && target == "rust" => {
                oddlang::integrity_block::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
            }
        [cmd, kind, target, graph, overlay, output] if cmd == "build" && kind == "shared-declarations" && target == "rust" => {
            oddlang::shared_declarations::build(graph.as_ref(), overlay.as_ref(), output.as_ref())
        }
        [cmd, kind, target, manifest, output] if cmd == "build" && kind == "native-package" && target == "rust" => {
            oddlang::native_package::build(manifest.as_ref(), output.as_ref())
        }
        [cmd, kind, target, root, overlay, out] if cmd == "build" && kind == "json-envelope" && target == "rust" => {
            oddlang::json_envelope::build(root.as_ref(), overlay.as_ref(), out.as_ref())
        }
        [cmd, kind, target, root, overlay, release]
            if cmd == "verify" && kind == "value-domain" && target == "rust" => {
                oddlang::value_domain::verify(root.as_ref(), Some(overlay.as_ref()), release.as_ref())
        }
        [cmd, kind, target, root, release]
            if cmd == "verify" && kind == "value-domain" && target == "typescript" => {
                oddlang::value_domain::verify(root.as_ref(), None, release.as_ref())
        }
        [cmd, kind, manifest, selection, out] if cmd == "recover" && kind == "value-domain" => {
            oddlang::value_domain::recover::recover(manifest.as_ref(), selection.as_ref(), out.as_ref())
        }
        [cmd, kind, target, root, out] if cmd == "build" && kind == "value-domain" && target == "typescript" => {
            oddlang::value_domain::build_typescript(root.as_ref(), out.as_ref())
        }
        [cmd, kind, target, root, overlay, out] if cmd == "build" && kind == "value-domain" && target == "rust" => {
            oddlang::value_domain::build(root.as_ref(), overlay.as_ref(), out.as_ref())
        }
        [group, command, graph] if group == "sequence-journal" && command == "digest" => {
            println!("{}", oddlang::sequence_journal::digest(graph.as_ref())?);
            Ok(())
        }
        [group, command, overlay] if group == "sequence-journal" && command == "overlay-digest" => {
            println!("{}", oddlang::sequence_journal::overlay_digest(overlay.as_ref())?);
            Ok(())
        }
        [command, kind, target, graph, overlay, output, graph_flag, graph_pin, overlay_flag, overlay_pin]
            if command == "build" && kind == "sequence-journal" && target == "rust"
                && graph_flag == "--graph-sha256" && overlay_flag == "--overlay-sha256" =>
        {
            oddlang::sequence_journal::build(
                graph.as_ref(), graph_pin, overlay.as_ref(), overlay_pin, output.as_ref(),
            )
        }
        [group, command, graph] if group == "endpoint-set" && command == "digest" => {
            println!("{}", oddlang::endpoint_set::digest(graph.as_ref())?);
            Ok(())
        }
        [group, command, overlay] if group == "endpoint-set" && command == "overlay-digest" => {
            println!("{}", oddlang::endpoint_set::overlay_digest(overlay.as_ref())?);
            Ok(())
        }
        [command, kind, target, graph, output, flag, pin]
            if command == "build" && kind == "endpoint-set" && target == "rust"
                && flag == "--graph-sha256" =>
        {
            oddlang::endpoint_set::build(graph.as_ref(), pin, None, output.as_ref())
        }
        [command, kind, target, graph, overlay, output, graph_flag, graph_pin, overlay_flag, overlay_pin]
            if command == "build" && kind == "endpoint-set" && target == "rust"
                && graph_flag == "--graph-sha256" && overlay_flag == "--overlay-sha256" =>
        {
            oddlang::endpoint_set::build(
                graph.as_ref(), graph_pin, Some((overlay.as_ref(), overlay_pin)), output.as_ref(),
            )
        }
        [group, kind, source, selection, out] if group == "recover" && kind == "cli-surface" => {
            oddlang::command_surface::recover(source.as_ref(), selection.as_ref(), out.as_ref())
        }
        [group, kind, target, graph, binding, out]
            if group == "build" && kind == "cli-surface" && target == "rust" =>
        {
            oddlang::command_surface::build(graph.as_ref(), binding.as_ref(), out.as_ref())
        }
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
        [group, kind, manifest, profile_flag, profile, out_flag, output]
            if group == "recover" && kind == "rust-shapes" && profile_flag == "--profile" && out_flag == "--out" =>
        {
            oddlang::rust_recovery::recover_shapes(manifest.as_ref(), profile.as_ref(), output.as_ref())
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
        _ => Err("usage: oddlang recover build-report ROOT SELECTION OUTPUT | recover fixed-bytes OBSERVATION SELECTION OUTPUT | recover finite-error-domain SOURCE SELECTION OUTPUT | build finite-error-domain rust GRAPH OVERLAY OUTPUT | recover record-writer SOURCE SELECTION OUTPUT | recover wire-schema SOURCE SELECTION OUTPUT | build wire-schema rust GRAPH OVERLAY OUTPUT | build wire-protocol rust SHAPE SHAPE_BINDING GRAPH OVERLAY OUTPUT | check PROJECT | impact PROJECT IRI | build PROJECT OUTPUT | recover rust MANIFEST --profile PROFILE --out OUTPUT | recover rust-shapes MANIFEST --profile PROFILE --out OUTPUT | recover tagged-contract SOURCE SELECTION OVERLAY VARIANT OUTPUT | build binary-frame rust GRAPH OVERLAY OUTPUT | build file-profile rust GRAPH BINDING OUTPUT | build binary-record rust GRAPH OVERLAY OUTPUT | build checked-values rust GRAPH OVERLAY OUTPUT | build field-relation rust GRAPH OVERLAY OUTPUT | build shared-declarations rust GRAPH OVERLAY OUTPUT | build integrity-block rust GRAPH OVERLAY OUTPUT | build local-status-owner rust GRAPH BINDING OUTPUT | build local-command-client rust GRAPH BINDING OUTPUT | recover cli-surface SOURCE SELECTION GRAPH | build cli-surface rust GRAPH BINDING OUTPUT | build native-package rust MANIFEST OUTPUT | build native-workspace rust MANIFEST OUTPUT | build json-envelope rust GRAPH OVERLAY OUTPUT | sequence-journal digest GRAPH | sequence-journal overlay-digest OVERLAY | build sequence-journal rust GRAPH OVERLAY OUTPUT --graph-sha256 PIN --overlay-sha256 PIN | endpoint-set digest GRAPH | endpoint-set overlay-digest OVERLAY | build endpoint-set rust GRAPH [OVERLAY] OUTPUT --graph-sha256 PIN [--overlay-sha256 PIN] | tagged-wire digest GRAPH | build tagged-wire TARGET GRAPH OUTPUT --graph-sha256 PIN | import rust-tree MANIFEST PACKAGE OUTPUT | build rust-tree BUNDLE OUTPUT --bundle-sha256 PIN".into()),
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
