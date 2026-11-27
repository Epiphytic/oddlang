use super::*;
pub struct Recovery<'a> {
    pub root: &'a Path,
    pub upstream: &'a Path,
    pub binary: &'a Path,
}
impl Recovery<'_> {
    fn isolated(&self, hidden: bool) -> Result<Command> {
        if hidden {
            let mut c = Command::new("sandbox-exec");
            c.arg("-D").arg(format!("ORIGINAL={}",self.upstream.display())).args(["-p","(version 1)(allow default)(deny network-outbound (remote ip \"*:*\"))(deny network-inbound (local ip \"*:*\"))(deny file-read* (subpath (param \"ORIGINAL\")))"]);
            Ok(c)
        } else {
            Ok(offline::command()?)
        }
    }
    fn cli(&self, stage: &Path, hidden: bool, args: &[&OsStr]) -> Result<()> {
        output(
            self.isolated(hidden)?
                .current_dir(stage)
                .arg(self.binary)
                .args(args),
        )?;
        Ok(())
    }
    pub fn assemble(&self, destination: &Path, evidence: &Path) -> Result<()> {
        let tmp = tempfile::Builder::new()
            .prefix("oddlang-recover-")
            .tempdir()?;
        let stage = tmp.path();
        let original = self.upstream.join("crates/marmish-core/Cargo.toml");
        let check = self
            .isolated(true)?
            .arg("/bin/cat")
            .arg(&original)
            .output()?;
        ensure!(
            original.is_file()
                && check.status.code() == Some(1)
                && check.stdout.is_empty()
                && String::from_utf8_lossy(&check.stderr).contains("Operation not permitted"),
            "projection boundary did not deny original source"
        );
        let releases = self.root.join("examples/marmish");
        for name in ["framing", "facade"] {
            let mapping = releases.join(format!("{name}.toml"));
            let imported = evidence.join(name);
            let projected = evidence.join(format!("{name}-projection"));
            let release = read_json(&releases.join(format!("{name}-release.json")))?;
            let pin = OsStr::new(text(&release["importSha256"])?);
            for op in ["rust", "replay"] {
                self.cli(
                    stage,
                    false,
                    &[
                        "import".as_ref(),
                        op.as_ref(),
                        original.as_os_str(),
                        mapping.as_os_str(),
                        imported.as_os_str(),
                    ],
                )?;
            }
            self.cli(
                stage,
                true,
                &[
                    "import".as_ref(),
                    "verify".as_ref(),
                    imported.as_os_str(),
                    "--import-sha256".as_ref(),
                    pin,
                ],
            )?;
            self.cli(
                stage,
                true,
                &[
                    "build".as_ref(),
                    "module".as_ref(),
                    imported.as_os_str(),
                    projected.as_os_str(),
                    "--import-sha256".as_ref(),
                    pin,
                ],
            )?;
        }
        let prepared = stage.join("prepared");
        output(
            Command::new("cargo")
                .current_dir(self.root)
                .args([
                    "+1.97.1",
                    "run",
                    "--quiet",
                    "--locked",
                    "--offline",
                    "--manifest-path",
                    "verification/provider-validation/Cargo.toml",
                    "--bin",
                    "marmish-prepare",
                    "--",
                    "v2",
                ])
                .arg(self.upstream)
                .args(
                    [
                        "framing",
                        "framing-projection",
                        "facade",
                        "facade-projection",
                    ]
                    .map(|n| evidence.join(n)),
                )
                .arg(&prepared),
        )?;
        let manifest = prepared.join("project.toml");
        let bytes = read(&manifest)?;
        let pin = fs::read_to_string(releases.join("assembly-v2.sha256"))?;
        ensure!(
            Some(digest(&bytes).as_str()) == pin.split_whitespace().next(),
            "workspace plan differs from the approved multi-producer manifest"
        );
        self.cli(
            stage,
            true,
            &[
                "assemble".as_ref(),
                manifest.as_os_str(),
                prepared.join("store").as_os_str(),
                destination.as_os_str(),
            ],
        )?;
        fs::write(evidence.join("project.toml"), bytes)?;
        Ok(())
    }
}
