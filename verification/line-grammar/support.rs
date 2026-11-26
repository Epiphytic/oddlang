#[derive(Debug)]
enum Fault {
    Invalid(&'static str),
}

mod generated {
    include!(env!("GENERATED"));
}

fn main() {
    use generated::{Config, TIERS, is_branch};
    const ORDER: [&str; 2] = TIERS;
    assert_eq!(ORDER, ["bronze", "copper"]);
    let validator: fn(&str) -> bool = is_branch;
    assert!(validator("b65535"));
    assert!(!validator("b065535"));
    assert_eq!(Config::parse(b"branch b1\ntier copper\n").unwrap().tier, "copper");
    let Err(Fault::Invalid(code)) = Config::parse(b"branch b1\ntier iron\n") else {
        panic!("unexpectedly parsed a tier outside the native constant");
    };
    assert_eq!(code, "invalid_configuration");
}
