pub mod bundle;
pub mod diff;
pub mod model;
pub mod runner;
pub mod wit;

/// Dispatch the component contract subcommands of the public Oddlang CLI.
pub fn command(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, source, lock, cache, out] if cmd == "publish" => {
            bundle::publish(source.as_ref(), lock.as_ref(), cache.as_ref(), out.as_ref())
        }
        [cmd, wit, world, identities, out] if cmd == "import-wit" => {
            wit::recover(wit.as_ref(), world, identities, out.as_ref())
        }
        [cmd, wit, world, identities, out, flag, lock] if cmd == "import-wit" && flag == "--lock" => {
            wit::recover_locked(wit.as_ref(), world, identities, out.as_ref(), Some(lock.as_ref()))
        }
        [cmd, old, old_lock, new, new_lock, direction, format] if cmd == "diff" => {
            let direction = match direction.as_str() {
                "old-host-new-guest" => diff::Direction::OldHostNewGuest,
                "new-host-old-guest" => diff::Direction::NewHostOldGuest,
                other => return Err(format!("unknown host/guest direction {other}")),
            };
            let old = bundle::verify_bundle(old.as_ref(), old_lock.as_ref())?;
            let new = bundle::verify_bundle(new.as_ref(), new_lock.as_ref())?;
            let report = diff::compare(&old.world, &new.world, direction)?;
            match format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?),
                "text" => println!("{report}"),
                other => return Err(format!("unknown diff format {other}")),
            }
            Ok(())
        }
        [cmd,bundle,lock,claim,component,out] if cmd=="run" => runner::run(bundle.as_ref(),lock.as_ref(),claim.as_ref(),component.as_ref(),out.as_ref()),
        [cmd,bundle,lock,claim,evidence] if cmd=="verify-evidence" => runner::verify(bundle.as_ref(),lock.as_ref(),claim.as_ref(),evidence.as_ref()),
        [cmd,host,hl,guest,gl,claim,component,direction,out] if cmd=="matrix" => runner::matrix((host.as_ref(),hl.as_ref()),(guest.as_ref(),gl.as_ref()),claim.as_ref(),component.as_ref(),direction,out.as_ref()),
        _ => Err("usage: oddlang wasm publish SOURCE DEPENDENCY_LOCK CACHE OUT | import-wit WIT_DIR WORLD ID_MAP_OR_NAMESPACE OUT [--lock WORKSPACE_LOCK] | diff OLD OLD_LOCK NEW NEW_LOCK DIRECTION json|text | run BUNDLE LOCK CLAIM COMPONENT OUT | verify-evidence BUNDLE LOCK CLAIM EVIDENCE | matrix HOST HOST_LOCK GUEST GUEST_LOCK CLAIM COMPONENT DIRECTION OUT".into()),
    }
}
