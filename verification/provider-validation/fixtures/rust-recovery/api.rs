
mod detail {
    #[derive(Clone, Copy)]
    pub enum Phase { Idle, Busy }
    pub struct Sample(u16);
    impl Sample {
        pub fn read(&self, pressure: &[u8]) -> u16 { self.0 + pressure.len() as u16 }
        pub const LIMIT: u16 = 11;
    }
}
pub use detail::{Phase, Sample};
