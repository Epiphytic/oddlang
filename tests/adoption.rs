use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn run(cmd: &mut Command, expected_error: Option<&str>) -> Result<String> {
    cmd.envs([
        ("GOENV", "off"),
        ("GOWORK", "off"),
        ("GOPROXY", "off"),
        ("GOSUMDB", "off"),
        ("GOTOOLCHAIN", "local"),
        ("GOFLAGS", "-mod=readonly"),
    ]);
    let output = cmd.output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let ok = match expected_error {
        Some(message) => !output.status.success() && stderr.contains(message),
        None => output.status.success(),
    };
    if !ok {
        return Err(format!(
            "{cmd:?}: {}: {stderr} {}",
            output.status,
            String::from_utf8_lossy(&output.stdout)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

struct Consumer<'a> {
    dir: PathBuf,
    fixture: &'a Path,
    replay: &'a Path,
}

impl<'a> Consumer<'a> {
    fn new(dir: PathBuf, fixture: &'a Path, replay: &'a Path) -> Result<Self> {
        fs::create_dir(&dir)?;
        for file in ["go.mod", "client_test.go", "lock_test.go"] {
            fs::copy(fixture.join(file), dir.join(file))?;
        }
        Ok(Self {
            dir,
            fixture,
            replay,
        })
    }

    fn replay(&self, args: &[&Path], error: Option<&str>) -> Result<String> {
        run(
            Command::new(self.replay).current_dir(&self.dir).args(args),
            error,
        )
    }

    fn install(&self, contracts: &Path, client_lock: &Path, version: &str) -> Result {
        fs::copy(client_lock, self.dir.join("client.lock.json"))?;
        fs::copy(
            self.fixture
                .join(format!("pins/target-{version}.lock.json")),
            self.dir.join("target.lock.json"),
        )?;
        let restored = self.replay(
            &[Path::new("restore"), &self.fixture.join("revisions")],
            None,
        )?;
        assert_eq!(restored.trim(), version);
        let bundle = self.dir.join("bundle");
        if bundle.try_exists()? {
            fs::remove_dir_all(&bundle)?;
        }
        copy_tree(&contracts.join("cache/ledger").join(version), &bundle)
    }

    fn test(&self, version: &str) -> Result {
        self.replay(&[Path::new("verify")], None)?;
        run(
            Command::new("go")
                .current_dir(&self.dir)
                .env("EXPECTED_VERSION", version)
                .args([
                    "test",
                    "-count=1",
                    "client.go",
                    "client_test.go",
                    "lock_test.go",
                ]),
            None,
        )?;
        Ok(())
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
fn independent_go_adoption() -> Result {
    let fixture = Path::new("examples/contracts/go-client").canonicalize()?;
    let contracts = Path::new("contracts").canonicalize()?;
    let scratch = tempfile::tempdir()?;
    let replay = scratch.path().join("replay");
    run(
        Command::new("go")
            .current_dir(&fixture)
            .args(["build", "-o"])
            .arg(&replay)
            .arg("tools/replay.go"),
        None,
    )?;
    let publisher = Consumer::new(scratch.path().join("publisher"), &fixture, &replay)?;
    let client = Consumer::new(scratch.path().join("client"), &fixture, &replay)?;
    for version in ["v1", "v2"] {
        publisher.install(
            &contracts,
            &contracts.join(format!("workspace-{version}.lock.json")),
            version,
        )?;
        publisher.test(version)?;
        if version == "v1" {
            client.install(&contracts, &fixture.join("pins/client-v1.lock.json"), "v1")?;
        }
        client.test("v1")?;
    }
    client.install(&contracts, &fixture.join("pins/client-v2.lock.json"), "v2")?;
    client.test("v2")?;
    fs::OpenOptions::new()
        .append(true)
        .open(client.dir.join("client.go"))?
        .write_all(b"\n// changed after review\n")?;
    client.replay(
        &[Path::new("verify")],
        Some("digest mismatch for client.go"),
    )?;
    client.replay(&[Path::new("restore"), &fixture.join("revisions")], None)?;
    let extra = client.dir.join("extra.go");
    fs::write(&extra, "package client\n")?;
    client.replay(&[Path::new("verify")], Some("unreviewed source extra.go"))?;
    fs::remove_file(extra)?;
    client.test("v2")
}
