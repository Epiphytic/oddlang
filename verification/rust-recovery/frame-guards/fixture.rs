const WINDOW: u64 = 9;
const CHUNK: usize = 4;

#[derive(Debug)]
pub enum Fault { Capacity, End, Header, Length, Tag, Truncated, Trailing, DecoderFailed }

pub enum Parcel {
    Chunk { seq: u64, contents: Vec<u8> },
}

impl Parcel {
    pub fn encode(&self) -> Result<Vec<u8>, Fault> {
        let mut payload = Vec::new();
        let tag = match self {
            Self::Chunk { seq: number, contents } => {
                if contents.is_empty() || contents.len() > CHUNK {
                    return Err(Fault::Capacity);
                }
                upper(*number, contents.len())?;
                payload.extend((*number).to_be_bytes());
                payload.extend(contents);
                43
            }
        };
        if payload.len() > 64 {
            return Err(Fault::Capacity);
        }
        let mut bytes = Vec::with_capacity(payload.len() + 5);
        bytes.push(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(payload);
        Ok(bytes)
    }
}

fn upper(sequence: u64, count: usize) -> Result<u64, Fault> {
    sequence.checked_add(count as u64).filter(|end| *end <= WINDOW).ok_or(Fault::End)
}
