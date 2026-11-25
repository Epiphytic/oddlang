use ledger_consumers::{decode, encode, read_input};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let account = decode(&read_input()?)?;
    println!("{}", encode(&account));
    Ok(())
}
