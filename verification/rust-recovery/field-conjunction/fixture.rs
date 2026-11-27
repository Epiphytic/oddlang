
mod types;
use types::Stage;

pub struct Invoice { pub serial: u32, pub flags: Vec<u8>, pub stage: Stage }
pub struct Receipt { pub serial: u32, pub backup: u32, pub flags: Vec<u8>, pub stage: Stage }

impl Invoice {
    pub fn agrees(&self, other: &Receipt) -> bool {
        other.serial == self.serial && self.flags == other.flags && self.stage == other.stage
    }
}
