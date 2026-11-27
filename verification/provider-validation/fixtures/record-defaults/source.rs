use std::path::PathBuf;
macro_rules! unit { ($name:ident) => {struct $name;} }
unit!(Hidden);

#[derive(Clone, Debug, Default)]
pub struct CartographicSample {
    pub waypoints: u32,
    pub points: u64,
    pub current: bool,
    pub reference: PathBuf,
}
