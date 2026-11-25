use ledger_consumers::{TRANSFER, decode, encode, read_input};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = read_input()?;
    let reply = match std::env::args().nth(1).as_deref() {
        Some("account") => encode(&decode(&input)?),
        Some("transfer") => TRANSFER.ok_or("unknown endpoint")?(&input)?,
        _ => return Err("unknown endpoint".into()),
    };
    println!("{reply}");
    Ok(())
}
