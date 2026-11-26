use roundtrip_contract_probe::{Measure, Observation, observe};

struct Triple(i32);

impl Measure for Triple {
    fn measure(&self) -> i32 {
        3 * self.0
    }
}

#[test]
fn observes_the_pinned_native_contract() {
    let actual = observe(&Triple(5));

    assert_eq!(
        actual,
        Observation {
            result: if cfg!(feature = "extra") { 54 } else { 29 },
            punctuation: (1, 2, 3),
            marker: "prefix \"# embedded",
            crlf: "first\nsecond",
            bom: "café",
            line: 13,
            file: actual.file,
        }
    );
    assert!(
        actual.file.ends_with("src/lib.rs"),
        "unexpected source location: {}",
        actual.file
    );
}

#[test]
fn runs_the_target_with_a_shebang() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_shebang"))
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"edition-2024 shebang\n");
    assert!(result.stderr.is_empty());
}
