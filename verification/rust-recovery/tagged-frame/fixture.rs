pub enum Packet {
    Key { first: [u8; 2], second: [u8; 2] },
    Reading { value: u64, footer: [u8; 2] },
}

impl Packet {
    pub fn encode(&self) -> Result<Vec<u8>, &'static str> {
        let mut payload = Vec::new();
        let tag = match self {
            Self::Key { first: a, second } => {
                payload.extend(a);
                payload.extend(second);
                31
            }
            Self::Reading { value, footer } => {
                append_reading(&mut payload, *value, footer);
                payload.push(84);
                32
            }
        };
        if payload.len() > 32 {
            return Err("too long");
        }
        let mut bytes = Vec::with_capacity(payload.len() + 5);
        bytes.push(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(payload);
        Ok(bytes)
    }
}

fn append_reading(out: &mut Vec<u8>, data: u64, footer: &[u8; 2]) {
    out.extend(data.to_be_bytes());
    out.extend(footer);
}
