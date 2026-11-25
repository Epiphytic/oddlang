use serde_json::Value;

pub mod account {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../generated/account.rs"
    ));
}
use account::Account;
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../generated/adapter.rs"
));

fn text(value: &Value) -> Result<&str, String> {
    value.as_str().ok_or_else(|| "expected JSON string".into())
}

fn decimal(value: &Value) -> Result<i128, String> {
    let s = text(value)?;
    let n = s.parse::<i64>().map_err(|_| "invalid i64 decimal")?;
    if n.to_string() != s {
        return Err("expected canonical decimal".into());
    }
    Ok(n.into())
}

pub fn read_input() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(std::io::stdin().lock())?)
}
