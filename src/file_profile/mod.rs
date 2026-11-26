//! Native projection of a private file-backed credential profile.
use crate::local_command_client::{Callback, RequiredArgument};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    format: String,
    id: String,
    arguments: Arguments,
    layout: Layout,
    manifest: Manifest,
    errors: Errors,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    state: String,
    profile: String,
    backend: String,
    default_profile: String,
    file_backend: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    owner: String,
    manifest: String,
    keys: String,
    private_subdirectories: Vec<String>,
    signing_prefix: String,
    storage_prefix: String,
    transaction_prefix: String,
    name_maximum_bytes: u16,
    name_punctuation: String,
    manifest_maximum_bytes: u32,
    directory_mode: u16,
    file_mode: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    pending: String,
    ready: String,
    file_backend: String,
    other_backends: Vec<String>,
    fields: Fields,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    version: String,
    profile: String,
    name: String,
    transaction: String,
    phase: String,
    backend: String,
    signer: String,
    storage: String,
    slot: String,
    public: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    exit: u8,
    stderr: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Errors {
    binding: Diagnostic,
    state_required: Diagnostic,
    backend_required: Diagnostic,
    unsafe_directory: Diagnostic,
    unsafe_file: Diagnostic,
    invalid_profile: Diagnostic,
    in_use: Diagnostic,
    invalid_manifest: Diagnostic,
    wrong_version: Diagnostic,
    wrong_backend: Diagnostic,
    random: Diagnostic,
    missing_secret: Diagnostic,
    invalid_secret: Diagnostic,
    invalid_signing_key: Diagnostic,
    identity_mismatch: Diagnostic,
    key_readback: Diagnostic,
    io: Diagnostic,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    format: String,
    entry: String,
    random: RandomProvider,
    signing: SigningProvider,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RandomProvider {
    Getrandom,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SigningProvider {
    Secp256k1XOnly,
}

fn component(value: &str) -> bool {
    !value.is_empty() && !value.contains(['/', '\0']) && value != "." && value != ".."
}

fn distinct<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .all(|value| !value.is_empty() && seen.insert(value))
}

fn argument(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

fn load(graph: &[u8], binding: &[u8]) -> Result<(Policy, Binding), String> {
    let graph: Policy = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let binding: Binding = serde_json::from_slice(binding).map_err(|error| error.to_string())?;
    oxigraph::model::NamedNode::new(&graph.id).map_err(|_| "invalid profile policy identity")?;
    if graph.format != "oddlang-file-profile-v1"
        || binding.format != "oddlang-file-profile-rust-v1"
        || !matches!(binding.random, RandomProvider::Getrandom)
        || !matches!(binding.signing, SigningProvider::Secp256k1XOnly)
        || !binding
            .entry
            .starts_with(|ch: char| ch.is_ascii_lowercase())
        || syn::parse_str::<syn::Ident>(&binding.entry).is_err()
        || binding.entry == "gen"
        || binding.entry.starts_with("profile_")
    {
        return Err("invalid file profile projection binding".into());
    }
    let Arguments {
        state,
        profile,
        backend,
        default_profile,
        file_backend,
    } = &graph.arguments;
    if ![state, profile, backend]
        .into_iter()
        .all(|name| argument(name))
        || !distinct([state.as_str(), profile.as_str(), backend.as_str()])
        || !component(default_profile)
        || !component(file_backend)
        || default_profile.len() > usize::from(graph.layout.name_maximum_bytes)
        || !default_profile.bytes().all(|b| {
            b.is_ascii_alphanumeric() || graph.layout.name_punctuation.as_bytes().contains(&b)
        })
    {
        return Err("invalid file profile arguments".into());
    }
    let shape = &graph.layout;
    let children = [
        shape.owner.as_str(),
        shape.manifest.as_str(),
        shape.keys.as_str(),
    ]
    .into_iter()
    .chain(shape.private_subdirectories.iter().map(String::as_str));
    if !children.clone().all(component)
        || !distinct(children)
        || ![
            &shape.signing_prefix,
            &shape.storage_prefix,
            &shape.transaction_prefix,
        ]
        .into_iter()
        .all(|part| component(part))
        || !distinct([
            shape.signing_prefix.as_str(),
            shape.storage_prefix.as_str(),
            shape.transaction_prefix.as_str(),
        ])
        || shape.name_maximum_bytes == 0
        || !(1..=16_777_216).contains(&shape.manifest_maximum_bytes)
        || !shape
            .name_punctuation
            .bytes()
            .all(|byte| byte.is_ascii_punctuation() && byte != b'/')
        || shape.directory_mode != 0o700
        || shape.file_mode != 0o600
        || [
            shape.owner.as_str(),
            shape.manifest.as_str(),
            shape.keys.as_str(),
        ]
        .into_iter()
        .chain(shape.private_subdirectories.iter().map(String::as_str))
        .any(|name| name.starts_with(&shape.transaction_prefix))
    {
        return Err("invalid private file profile layout".into());
    }
    let manifest = &graph.manifest;
    let fields = &manifest.fields;
    if !distinct([manifest.pending.as_str(), manifest.ready.as_str()])
        || !distinct(
            std::iter::once(manifest.file_backend.as_str())
                .chain(manifest.other_backends.iter().map(String::as_str)),
        )
        || !distinct(
            [
                &fields.version,
                &fields.profile,
                &fields.name,
                &fields.transaction,
                &fields.phase,
                &fields.backend,
                &fields.signer,
                &fields.storage,
                &fields.slot,
                &fields.public,
            ]
            .into_iter()
            .map(String::as_str),
        )
    {
        return Err("invalid private profile record".into());
    }
    for (_, error) in graph.errors.iter() {
        if error.exit == 0 || error.stderr.is_empty() || error.stderr.chars().any(char::is_control)
        {
            return Err("invalid file profile diagnostic".into());
        }
    }
    Ok((graph, binding))
}

impl Errors {
    fn iter(&self) -> [(&'static str, &Diagnostic); 17] {
        [
            ("BINDING", &self.binding),
            ("STATE_REQUIRED", &self.state_required),
            ("BACKEND_REQUIRED", &self.backend_required),
            ("UNSAFE_DIRECTORY", &self.unsafe_directory),
            ("UNSAFE_FILE", &self.unsafe_file),
            ("INVALID_PROFILE", &self.invalid_profile),
            ("IN_USE", &self.in_use),
            ("INVALID_MANIFEST", &self.invalid_manifest),
            ("WRONG_VERSION", &self.wrong_version),
            ("WRONG_BACKEND", &self.wrong_backend),
            ("RANDOM", &self.random),
            ("MISSING_SECRET", &self.missing_secret),
            ("INVALID_SECRET", &self.invalid_secret),
            ("INVALID_SIGNING_KEY", &self.invalid_signing_key),
            ("IDENTITY_MISMATCH", &self.identity_mismatch),
            ("KEY_READBACK", &self.key_readback),
            ("IO", &self.io),
        ]
    }
}

pub(crate) fn callback(graph: &[u8], binding: &[u8]) -> Result<Callback, String> {
    let (policy, native) = load(graph, binding)?;
    Ok(Callback {
        entry: native.entry,
        arguments: vec![
            (policy.arguments.state, RequiredArgument::Path),
            (policy.arguments.profile, RequiredArgument::String),
            (policy.arguments.backend, RequiredArgument::String),
        ],
    })
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<(), String> {
    let read = |path| fs::read(path).map_err(|error| error.to_string());
    build_bytes(&read(graph)?, &read(binding)?, output)
}

pub(crate) fn build_bytes(graph: &[u8], binding: &[u8], output: &Path) -> Result<(), String> {
    let (policy, native) = load(graph, binding)?;
    let source = render(&policy, &native);
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let obligations = serde_json::json!({
        "format":"oddlang-file-profile-obligations-v1", "policy":policy,
        "effectOrder":["explicit-file-provider", "create-private-state", "canonical-private-profile",
            "exclusive-profile-lease", "closed-bounded-manifest", "pending-manifest", "load-or-create-signing-key",
            "check-derived-public", "update-pending-public-binding", "load-or-create-storage-key", "private-directories"],
        "completion":"credential-preparation-only",
        "qualification":"independent-witness-required",
    });
    let catalog = format!("{obligations:#}\n");
    let lock = serde_json::json!({"format":"oddlang-file-profile-lock-v1","graphSha256":hash(graph),
        "bindingSha256":hash(binding),"sourceSha256":hash(source.as_bytes()),"obligationsSha256":hash(catalog.as_bytes())});
    if output.symlink_metadata().is_ok() {
        return Err("file profile output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    for (name, content) in [
        ("profile.rs", source),
        ("profile-obligations.json", catalog),
        ("projection.lock", format!("{lock:#}\n")),
    ] {
        fs::write(stage.path().join(name), content).map_err(|error| error.to_string())?;
    }
    crate::native_package::publish(stage.path(), output)
}

fn render(policy: &Policy, binding: &Binding) -> String {
    let mut out = String::from(include_str!("runtime.rs.in"));
    for (name, diagnostic) in policy.errors.iter() {
        writeln!(
            out,
            "const {name}: Failure = Failure({}, {:?});",
            diagnostic.exit, diagnostic.stderr
        )
        .unwrap();
    }
    let layout = &policy.layout;
    for (name, value) in [
        ("OWNER", &layout.owner),
        ("MANIFEST", &layout.manifest),
        ("KEYS", &layout.keys),
        ("SIGNING_PREFIX", &layout.signing_prefix),
        ("STORAGE_PREFIX", &layout.storage_prefix),
        ("TRANSACTION_PREFIX", &layout.transaction_prefix),
        ("PUNCTUATION", &layout.name_punctuation),
        ("PENDING", &policy.manifest.pending),
        ("READY", &policy.manifest.ready),
        ("FILES", &policy.manifest.file_backend),
    ] {
        writeln!(out, "const {name}: &str = {value:?};").unwrap();
    }
    for (name, value) in [
        ("NAME_MAX", u32::from(layout.name_maximum_bytes)),
        ("MANIFEST_MAX", layout.manifest_maximum_bytes),
        ("DIR_MODE", u32::from(layout.directory_mode)),
        ("FILE_MODE", u32::from(layout.file_mode)),
    ] {
        writeln!(out, "const {name}: u32 = {value};").unwrap();
    }
    writeln!(out, "const VERSION: u8 = {};\nconst EXTRA_DIRS: &[&str] = &{:?};\nconst OTHER_BACKENDS: &[&str] = &{:?};",
        policy.manifest.version, layout.private_subdirectories, policy.manifest.other_backends).unwrap();
    let fields = &policy.manifest.fields;
    out.push_str("#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(deny_unknown_fields)]\nstruct Record {\n");
    for (role, native, ty) in [
        (&fields.version, "version", "u8"),
        (&fields.profile, "profile", "String"),
        (&fields.name, "name", "String"),
        (&fields.transaction, "transaction", "String"),
        (&fields.phase, "phase", "String"),
        (&fields.backend, "backend", "String"),
        (&fields.signer, "signer", "String"),
        (&fields.storage, "storage", "String"),
        (&fields.slot, "slot", "String"),
        (&fields.public, "public", "Option<String>"),
    ] {
        writeln!(out, "    #[serde(rename = {role:?})]\n    {native}: {ty},").unwrap();
    }
    let arguments = &policy.arguments;
    writeln!(out, "}}
pub fn {}(matches: clap::ArgMatches) -> std::process::ExitCode {{
    let load = || -> Result<(), Failure> {{
        let base = matches.try_get_one::<std::path::PathBuf>({:?}).map_err(|_| BINDING)?.ok_or(STATE_REQUIRED)?;
        let name = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str).unwrap_or({:?});
        let backend = matches.try_get_one::<String>({:?}).map_err(|_| BINDING)?.map(String::as_str);
        if backend != Some({:?}) {{ return Err(BACKEND_REQUIRED); }}
        let profile = prepare(base, name)?;
        print_result(&profile)?;
        Ok(())
    }};
    match load() {{
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => error.report(),
    }}
}}
", binding.entry, arguments.state, arguments.profile, arguments.default_profile, arguments.backend, arguments.file_backend).unwrap();
    out
}
