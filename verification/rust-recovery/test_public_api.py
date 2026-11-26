"""Compare independently compiled native declarations for a small value API."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

TOOL = Path(sys.argv[1]).resolve()
SOURCE = """
mod detail {
    #[derive(Clone, Copy)]
    pub enum Phase { Idle, Busy }
    pub struct Sample(u16);
    impl Sample {
        pub fn read(&self, pressure: &[u8]) -> u16 { self.0 + pressure.len() as u16 }
        pub const LIMIT: u16 = 11;
    }
}
pub use detail::{Phase, Sample};
"""


def witness(work, source, index):
    lib = work / "lib.rs"
    lib.write_text(source)
    out = work / str(index)
    subprocess.run(["rustdoc", "+1.97.1", "-Z", "unstable-options", "--output-format", "json",
                    "--edition=2021", "--crate-name", "sensor", lib, "-o", out],
                   check=True, env=os.environ | {"RUSTC_BOOTSTRAP": "sensor"})
    return subprocess.run([TOOL, "inspect", "native-value-api", out / "sensor.json"],
                          capture_output=True, text=True)


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    first = witness(root, SOURCE, 0)
    first.check_returncode()
    facts = json.loads(first.stdout)
    assert list(facts["types"]) == ["Phase", "Sample"]
    renamed = SOURCE.replace("pressure", "bytes").replace("detail", "implementation")
    assert witness(root, renamed, 1).stdout == first.stdout
    for index, (before, after) in enumerate([
        ("Idle, Busy", "Busy, Idle"),
        ("&[u8]", "&mut [u8]"),
        ("Clone, Copy", "Clone"),
        ("const LIMIT: u16", "const LIMIT: u32"),
        ("pub fn read", "fn read"),
        ("-> u16 { self.0 + pressure.len() as u16 }",
         "-> u32 { u32::from(self.0) + pressure.len() as u32 }"),
    ], 2):
        probe = witness(root, SOURCE.replace(before, after), index)
        probe.check_returncode()
        assert json.loads(probe.stdout) != facts, (before, after)
    outside = witness(root, SOURCE.replace("pub struct Sample(u16)", "pub struct Sample(pub u16)"), 9)
    assert outside.returncode != 0 and "public tuple field" in outside.stderr
    print("two equivalent facades matched, six public changes distinguished, unsupported shape rejected")
