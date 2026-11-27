use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

pub struct ProofTools {
    pub prefix: PathBuf,
    pub kani_home: PathBuf,
    path: OsString,
}

impl ProofTools {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let prefix = env::var_os("ODDLANG_PROOF_TOOLS")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join("target/proof-tools"));
        let kani_home = match env::var_os("KANI_HOME").filter(|p| !p.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(env::var_os("HOME").ok_or("expected HOME or KANI_HOME")?)
                .join(".kani/kani-0.68.0"),
        };
        let path = env::join_paths(
            std::iter::once(prefix.join("bin"))
                .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
        )?;
        Ok(Self {
            prefix,
            kani_home,
            path,
        })
    }
    pub fn cargo(&self) -> Command {
        let mut cmd = Command::new("cargo");
        cmd.env("PATH", &self.path);
        cmd
    }
}
