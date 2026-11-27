//! Rebuild the two selected, import-free components with the installed Rust and wasm-tools.
use std::{env, fs, path::Path, process::Command};

fn run(command: &mut Command) {
    let status = command.status().expect("could not start build tool");
    assert!(status.success(), "{command:?} exited with {status}");
}

fn main() {
    let guest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = guest.parent().unwrap();
    let repo = root.parent().unwrap().parent().unwrap();
    let stage = repo.join("target/provider-guest");
    fs::create_dir_all(&stage).unwrap();
    let cargo_home = env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env::var_os("HOME").unwrap()).join(".cargo"));
    let flags = format!(
        "--remap-path-prefix={}=/provider --remap-path-prefix={}/registry/src=/registry",
        root.display(),
        cargo_home.display()
    );
    for world in ["generate", "recover"] {
        let mut cargo = Command::new("cargo");
        cargo
            .current_dir(guest)
            .env("RUSTFLAGS", &flags)
            .args([
                "build",
                "--locked",
                "--offline",
                "--release",
                "--lib",
                "--target",
                "wasm32-unknown-unknown",
                "--target-dir",
            ])
            .arg(&stage);
        if world == "recover" {
            cargo.args(["--features", "recover"]);
        }
        run(&mut cargo);
        let embedded = stage.join("embedded.wasm");
        run(Command::new("wasm-tools")
            .args(["component", "embed"])
            .arg(repo.join("contracts/providers/provider.wit"))
            .args(["--world", world])
            .arg(stage.join("wasm32-unknown-unknown/release/record_provider_example.wasm"))
            .arg("-o")
            .arg(&embedded));
        run(Command::new("wasm-tools")
            .args(["component", "new"])
            .arg(&embedded)
            .arg("-o")
            .arg(root.join(format!("{world}.wasm"))));
    }
}
