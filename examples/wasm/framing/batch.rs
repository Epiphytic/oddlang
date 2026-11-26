// Batch transport for the two independent clients. Neither side computes expectations.
use std::io::{self, Read, Write};

pub fn run(
    mut observe: impl FnMut(bool, &[u8]) -> Result<Result<Vec<u8>, String>, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut operation = [0];
    while input.read(&mut operation)? == 1 {
        let encode = match operation[0] {
            b'E' => true,
            b'D' => false,
            _ => return Err("unknown observation operation".into()),
        };
        let mut size = [0; 4];
        input.read_exact(&mut size)?;
        let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
        input.read_exact(&mut bytes)?;
        let (tag, payload) = match observe(encode, &bytes)? {
            Ok(bytes) => (b'B', bytes),
            Err(name) => (b'F', name.into_bytes()),
        };
        output.write_all(&[tag])?;
        output.write_all(&u32::try_from(payload.len())?.to_le_bytes())?;
        output.write_all(&payload)?;
    }
    Ok(())
}
