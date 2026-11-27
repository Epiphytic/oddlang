//! Exercise acquisition through a local Git URL rewrite and a disposable Cargo home.
use super::*;
pub(super) fn verify(cli: &Path, root: &Path) -> Result {
    fs::create_dir(root)?;
    let provider = root.join("provider");
    fs::create_dir_all(provider.join("src"))?;
    fs::write(
        provider.join("Cargo.toml"),
        "[package]\nname='reading'\nversion='0.1.0'\nedition='2021'\n",
    )?;
    fs::write(provider.join("src/lib.rs"), "pub const MAX: u8 = 9;\n")?;
    let git = |args: &[&str]| command(&mut Command::new("git"), &provider, args, None);
    git(&["init", "--quiet"])?;
    git(&["add", "src", "Cargo.toml"])?;
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgSign=false",
        "commit",
        "--quiet",
        "-m",
        "fixture",
    ])?;
    let rev = String::from_utf8(git(&["rev-parse", "HEAD"])?)?
        .trim()
        .to_owned();
    let url = "https://fixture.invalid/reading";
    let inputs = root.join("inputs");
    fs::create_dir(&inputs)?;
    make(&inputs, "instrument", "LIMIT", 9)?;
    let mut package = read(&inputs.join("instrument-package.json"))?;
    package["dependencies"]["reading"] =
        json!({"version":"0.1.0","source":{"git":url,"revision":rev}});
    fs::write(
        inputs.join("instrument.lock"),
        format!(
            "version=4\n[[package]]\nname='instrument'\nversion='0.1.0'\ndependencies=['reading']\n[[package]]\nname='reading'\nversion='0.1.0'\nsource='git+{url}?rev={rev}#{rev}'\n"
        ),
    )?;
    let lock = pin(&inputs, "instrument.lock")?;
    package["cargoLock"] = lock.clone();
    let package = put(&inputs, "instrument-package.json", &package)?;
    let mut workspace = json!({"format":"oddlang-native-workspace-v1","cargoLock":lock,"members":{"instrument":{"manifest":package}}});
    write(&inputs.join("workspace.json"), &workspace)?;
    let build = |dest: &str, fetch: bool, error: Option<&str>| -> Result {
        let mut cmd = Command::new(cli);
        cmd.current_dir(root)
            .env("RUSTUP_TOOLCHAIN", "1.97.1")
            .env("CARGO_HOME", root.join("cache"))
            .env_remove("CARGO_NET_OFFLINE")
            .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
            .env("GIT_CONFIG_COUNT", "1")
            .env(
                "GIT_CONFIG_KEY_0",
                format!("url.{}.insteadOf", provider.display()),
            )
            .env("GIT_CONFIG_VALUE_0", url)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args([
                "build",
                "native-workspace",
                "rust",
                "inputs/workspace.json",
                dest,
            ]);
        if fetch {
            cmd.arg("--fetch-locked");
        }
        let got = cmd.output()?;
        match error {
            Some(text) => {
                assert!(
                    !got.status.success() && String::from_utf8_lossy(&got.stderr).contains(text),
                    "{got:?}"
                );
                assert!(!root.join(dest).exists());
            }
            None => assert!(got.status.success(), "{got:?}"),
        }
        Ok(())
    };
    build("cold", false, Some("offline"))?;
    build("acquired", true, None)?;
    let disconnected = root.join("provider-disconnected");
    fs::rename(&provider, &disconnected)?;
    build("offline", false, None)?;
    assert_eq!(
        native_artifacts::inventory(&root.join("acquired"))?,
        native_artifacts::inventory(&root.join("offline"))?
    );
    assert_eq!(
        fs::read(root.join("offline/Cargo.lock"))?,
        fs::read(inputs.join("instrument.lock"))?
    );
    fs::rename(disconnected, &provider)?;
    fs::write(
        inputs.join("stale.lock"),
        "version=4\n[[package]]\nname='instrument'\nversion='0.1.0'\n",
    )?;
    workspace["cargoLock"] = pin(&inputs, "stale.lock")?;
    write(&inputs.join("workspace.json"), &workspace)?;
    build("rejected", true, Some("locked"))?;
    Ok(())
}
