//! Resolve a sealed recovery bundle through the existing native package pipeline.
use super::*;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum ComponentInput {
    Explicit(Component),
    Recovered(RecoveredInput),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveredInput {
    recovery: Pin,
    module: Option<String>,
    #[serde(rename = "sourceApi")]
    source_api: Option<Pin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryReceipt {
    format: String,
    graph_sha256: String,
    overlay_sha256: String,
    source_sha256: String,
    selection_sha256: String,
    crate_root_sha256: Option<String>,
    rustdoc_sha256: Option<String>,
}

pub(super) struct Provenance {
    pub(super) sha256: String,
    pub(super) declaration: String,
    pub(super) source_api: Option<Pin>,
}

fn sha256_digest(pin: &str) -> bool {
    pin.len() == 64
        && pin
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl RecoveredInput {
    fn resolve(self, base: &Path) -> Result<Component> {
        let recovery = self.recovery.read(base)?;
        let receipt: RecoveryReceipt = serde_json::from_slice(&recovery)
            .map_err(|error| format!("recovery receipt: {error}"))?;
        if !sha256_digest(&receipt.source_sha256) || !sha256_digest(&receipt.selection_sha256) {
            return Err("invalid source or selection digest in recovery receipt".into());
        }
        let directory = Path::new(&self.recovery.path)
            .parent()
            .ok_or("missing recovery receipt directory")?;
        let sibling = |name: &str, sha256: String| -> Result<Pin> {
            let path = directory.join(name);
            let path = path.to_str().ok_or("invalid recovery artifact path")?;
            Ok(Pin {
                path: path.into(),
                sha256,
            })
        };
        let graph = sibling("graph.json", receipt.graph_sha256)?;
        let overlay = sibling("rust.json", receipt.overlay_sha256)?;
        let graph_bytes = graph.read(base)?;
        let overlay_bytes = overlay.read(base)?;
        let (kind, module, name, identity) = match receipt.format.as_str() {
            "oddlang-serde-public-record-recovery-lock-v1" => {
                if !receipt
                    .crate_root_sha256
                    .as_deref()
                    .is_some_and(sha256_digest)
                    || !receipt.rustdoc_sha256.as_deref().is_some_and(sha256_digest)
                {
                    return Err(
                        "recovered public record requires crate-root and Rustdoc receipt pins"
                            .into(),
                    );
                }
                if self.module.is_some() {
                    return Err("public record recovery already determines its module".into());
                }
                let (module, name, identity) =
                    crate::public_records::recovered_component(&graph_bytes, &overlay_bytes)?;
                (Kind::PublicRecords, module, name, identity)
            }
            "oddlang-rust-serde-string-choice-recovery-v1" => {
                if self.source_api.is_some() {
                    return Err("sourceApi is only valid for recovered public records".into());
                }
                let module = self.module.ok_or(
                    "syntax-only choice recovery requires explicit module placement and root publication policy",
                )?;
                let (id, name) =
                    crate::shared_declarations::recovered_choice(&graph_bytes, &overlay_bytes)?;
                (Kind::SharedDeclarations, module, name, id)
            }
            _ => {
                return Err(format!(
                    "unsupported native recovery receipt: {}",
                    receipt.format
                ));
            }
        };
        Ok(Component {
            kind,
            module,
            graph,
            overlay,
            reexports: vec![name],
            wire_shape: None,
            visibility: Visibility::Private,
            root_reexports: None,
            crate_reexports: vec![],
            recovery: Some(Provenance {
                sha256: self.recovery.sha256,
                declaration: identity,
                source_api: self.source_api,
            }),
        })
    }
}

impl Manifest<ComponentInput> {
    pub(super) fn resolve(self, base: &Path) -> Result<Manifest> {
        let Manifest {
            format,
            name,
            version,
            edition,
            dependencies,
            registry_patches,
            cargo_lock,
            binaries,
            components,
        } = self;
        let mut identities = BTreeSet::new();
        let components = components
            .into_iter()
            .map(|input| match input {
                ComponentInput::Explicit(component) => Ok(component),
                ComponentInput::Recovered(recovered) => {
                    let component = recovered.resolve(base)?;
                    let id = &component
                        .recovery
                        .as_ref()
                        .expect("resolved recovery provenance")
                        .declaration;
                    if !identities.insert(id.clone()) {
                        return Err(format!("ambiguous recovered public declaration: {id}"));
                    }
                    Ok(component)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Manifest {
            format,
            name,
            version,
            edition,
            dependencies,
            registry_patches,
            cargo_lock,
            binaries,
            components,
        })
    }
}
