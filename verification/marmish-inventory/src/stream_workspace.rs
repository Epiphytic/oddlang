//! Host-side support for isolated stream workspace qualifications.
use crate::{
    PIN, digest, object, root,
    stream_baseline::{LOGICAL, RUST_COMMIT},
    text,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub type Environment = BTreeMap<String, String>;

pub fn output(cmd: &mut Command) -> Result<Vec<u8>> {
    let out = cmd.output().with_context(|| format!("start {cmd:?}"))?;
    ensure!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(out.stdout)
}
pub fn capture(cmd: &mut Command) -> Result<String> {
    Ok(String::from_utf8(output(cmd)?)?.trim().into())
}
pub fn save(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(fs::write(path, bytes)?)
}
pub fn git_archive(repo: &Path, commit: &str, out: &Path) -> Result<String> {
    ensure!(
        crate::valid_hash(&json!(commit), 40),
        "expected a full Git commit ID"
    );
    ensure!(
        capture(
            Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["rev-parse", &format!("{commit}^{{commit}}")])
        )? == commit,
        "revision is not a commit"
    );
    let bytes =
        output(
            Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["archive", "--format=tar", commit]),
        )?;
    fs::create_dir(out)?;
    let mut seen = BTreeSet::new();
    for entry in tar::Archive::new(bytes.as_slice()).entries()? {
        let entry = entry?;
        if entry.header().entry_type().is_pax_global_extensions() {
            continue;
        }
        let path = entry.path()?;
        let name = path
            .to_str()
            .context("non UTF-8 archive path")?
            .trim_end_matches('/');
        ensure!(
            !name.is_empty()
                && name
                    .split('/')
                    .all(|part| !matches!(part, "" | ".." | "." | ".git"))
                && seen.insert(name.to_owned()),
            "unsafe Git archive member {name}"
        );
        ensure!(
            entry.header().entry_type().is_dir() || entry.header().entry_type().is_file(),
            "special Git archive member {name}"
        );
    }
    for entry in tar::Archive::new(bytes.as_slice()).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_pax_global_extensions() {
            ensure!(entry.unpack_in(out)?, "archive member escaped output");
        }
    }
    Ok(digest(&bytes))
}
pub struct Tools {
    pub chain: PathBuf,
    pub sdk: PathBuf,
    pub clang: PathBuf,
    pub developer: PathBuf,
}
impl Tools {
    pub fn discover() -> Result<Self> {
        fn xcrun(args: &[&str]) -> Result<PathBuf> {
            Ok(fs::canonicalize(capture(
                Command::new("xcrun").args(args),
            )?)?)
        }
        let cargo = fs::canonicalize(capture(Command::new("rustup").args([
            "which",
            "--toolchain",
            "1.97.1",
            "cargo",
        ]))?)?;
        let chain = cargo
            .ancestors()
            .nth(2)
            .context("Rust toolchain")?
            .to_owned();
        ensure!(
            capture(Command::new(chain.join("bin/rustc")).arg("-vV"))?
                .contains(&format!("commit-hash: {RUST_COMMIT}")),
            "unexpected Rust compiler"
        );
        Ok(Self {
            chain,
            sdk: xcrun(&["--show-sdk-path"])?,
            clang: xcrun(&["--find", "clang"])?,
            developer: xcrun(&["--find", "make"])?
                .parent()
                .context("developer bin")?
                .to_owned(),
        })
    }
    pub fn boundaries(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut trees = vec![
            self.chain.clone(),
            self.sdk.clone(),
            self.clang.ancestors().nth(3).unwrap().to_owned(),
            self.developer.parent().unwrap().join("lib"),
        ];
        trees.extend(
            [
                "/dev/fd",
                "/System/Library",
                "/usr/lib",
                "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
                "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
            ]
            .map(PathBuf::from),
        );
        let mut files = vec![];
        for (base, names) in [
            ("/bin", "sh bash cat cp echo mkdir mv rm ln"),
            (
                "/usr/bin",
                "env git perl perl5.34 awk sed grep tr touch uname dirname basename find xargs cmp sort expr head tail mktemp diff",
            ),
        ] {
            files.extend(
                names
                    .split_whitespace()
                    .map(|name| Path::new(base).join(name)),
            );
        }
        files.extend([self.developer.join("make"), self.developer.join("git")]);
        files.extend(
            [
                "/",
                "/private/etc/ssl/openssl.cnf",
                "/dev/null",
                "/dev/zero",
                "/dev/urandom",
                "/dev/random",
            ]
            .map(PathBuf::from),
        );
        files.push(root().join("examples/marmish/stream-driver/main.rs"));
        (trees, files)
    }
    pub fn environment(&self, writable: &Path, src: &Path) -> Result<Environment> {
        fs::create_dir(writable)?;
        for name in ["home", "cargo", "tmp"] {
            fs::create_dir(writable.join(name))?;
        }
        let clang_bin = self.clang.parent().unwrap();
        let remap = format!("--remap-path-prefix={}={LOGICAL}", src.display());
        let val = json!({"HOME":writable.join("home"),"CARGO_HOME":writable.join("cargo"),"TMPDIR":writable.join("tmp"),"PATH":format!("{}/bin:{}:{}:/usr/bin:/bin",self.chain.display(),clang_bin.display(),self.developer.display()),"SDKROOT":self.sdk,"CC":self.clang,"CXX":clang_bin.join("clang++"),"RUSTC":self.chain.join("bin/rustc"), "RUSTDOC":self.chain.join("bin/rustdoc"),"CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER":self.clang,"CARGO_ENCODED_RUSTFLAGS":remap,"CARGO_ENCODED_RUSTDOCFLAGS":remap,"CARGO_NET_OFFLINE":"true","CARGO_TERM_COLOR":"never","OPENSSL_CONF":"/dev/null","MARMISH_BUILD_COMMIT":PIN,"LC_ALL":"C","TZ":"UTC"});
        Ok(serde_json::from_value(val)?)
    }
}
pub fn executable(package: &str, name: &str) -> Result<PathBuf> {
    let manifest = root().join("verification").join(package).join("Cargo.toml");
    let log = capture(
        Command::new("cargo")
            .args([
                "+1.97.1",
                "build",
                "--quiet",
                "--locked",
                "--offline",
                "--manifest-path",
            ])
            .arg(manifest)
            .args(["--bin", name, "--message-format=json-render-diagnostics"]),
    )?;
    let mut paths = vec![];
    for line in log.lines() {
        let row: Value = serde_json::from_str(line)?;
        if row["reason"] == "compiler-artifact"
            && row["target"]["name"] == name
            && let Some(exe) = row["executable"].as_str()
        {
            paths.push(PathBuf::from(exe));
        }
    }
    ensure!(paths.len() == 1, "missing native {name}");
    Ok(paths.remove(0))
}
pub fn closure(bin: &Path, req: Value) -> Result<Value> {
    let mut child = Command::new(bin)
        .arg("request")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&req)?)?;
    let out = child.wait_with_output()?;
    ensure!(out.status.success(), "build-closure {} failed", req["op"]);
    Ok(serde_json::from_slice(&out.stdout)?)
}
pub fn profile(
    bin: &Path,
    trees: &[PathBuf],
    writable: &Path,
    literals: &[PathBuf],
) -> Result<String> {
    Ok(text(&closure(
        bin,
        json!({"op":"profile","trees":trees,"writable":writable,"literals":literals}),
    )?)?
    .into())
}
pub fn denied_reads(sandbox: &Path, paths: Value, work: &Path, env: &Environment) -> Result<Value> {
    let mut rows = crate::Map::new();
    for (name, value) in object(&paths)? {
        let path = Path::new(text(value)?);
        let mut one = [0];
        ensure!(
            fs::File::open(path)?.read(&mut one)? == 1,
            "empty isolation canary {name}"
        );
        let probe = Command::new("/usr/bin/sandbox-exec")
            .arg("-f")
            .arg(sandbox)
            .arg("/bin/cat")
            .arg(path)
            .env_clear()
            .envs(env)
            .current_dir(work)
            .output()?;
        let err = String::from_utf8(probe.stderr)?;
        ensure!(
            !probe.status.success()
                && probe.stdout.is_empty()
                && err.contains("Operation not permitted"),
            "read isolation probe failed: {name}"
        );
        rows.insert(
            name.clone(),
            json!({"path":path,"returnCode":probe.status.code(),"stderr":err.trim()}),
        );
    }
    Ok(rows.into())
}
