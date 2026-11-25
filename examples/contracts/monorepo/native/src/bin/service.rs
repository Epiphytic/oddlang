use ledger_consumers::{decode, encode, read_input, transfer};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = read_input()?;
    let reply = match std::env::args().nth(1).as_deref() {
        Some("account") => encode(&decode(&input)?),
        Some("transfer") => transfer(&input)?,
        _ => return Err("expected account or transfer".into()),
    };
    println!("{reply}");
    Ok(())
}
