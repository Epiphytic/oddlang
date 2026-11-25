#[test]
fn independent_go_adoption() {
    let result = std::process::Command::new("sh")
        .arg("examples/contracts/go-client/verify.sh")
        .status()
        .expect("run isolated Go adoption fixture");
    assert!(result.success());
}
