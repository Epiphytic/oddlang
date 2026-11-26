#[derive(Debug)]
pub enum Fault { Invalid, Limit }

pub enum Envelope {
    Head(Head, Level),
    Copy(Copy),
}
#[derive(Clone, Copy)]
pub enum Level { Low = 1, High = 2 }
pub struct Head { pub first: [u8; 2], pub second: [u8; 2], pub version: u32 }
pub struct Copy { pub first: [u8; 2], pub second: [u8; 2], pub count: Option<u32> }

impl Head {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.version == 1 { Ok(()) } else { Err(Fault::Invalid) }
    }
}
impl Copy {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.count.is_some_and(|count| count & 1 == 0) { Err(Fault::Invalid) } else { Ok(()) }
    }
    pub fn encode(&self) -> Result<Vec<u8>, Fault> {
        self.validate()?;
        let mut bytes = Vec::new();
        append_copy(&mut bytes, self);
        Ok(bytes)
    }
}
impl Envelope {
    pub fn encode(&self) -> Result<Vec<u8>, Fault> {
        let mut payload = Vec::new();
        let tag = match self {
            Self::Head(head, level) => {
                head.validate()?;
                append_head(&mut payload, head);
                payload.push(*level as u8);
                13
            }
            Self::Copy(copy) => {
                payload = copy.encode()?;
                14
            }
        };
        if payload.len() > 64 {
            return Err(Fault::Limit);
        }
        let mut bytes = Vec::with_capacity(payload.len() + 5);
        bytes.push(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(payload);
        Ok(bytes)
    }
}

fn append_head(out: &mut Vec<u8>, head: &Head) {
    out.extend(head.first);
    out.extend(head.second);
    append32(out, head.version);
}
fn append_copy(out: &mut Vec<u8>, copy: &Copy) {
    out.extend(copy.first);
    out.extend(copy.second);
    marked(out, copy.count, append32);
}
fn append32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}
fn marked<T>(out: &mut Vec<u8>, value: Option<T>, append: impl FnOnce(&mut Vec<u8>, T)) {
    match value {
        Some(value) => { out.push(1); append(out, value); }
        None => out.push(0),
    }
}
