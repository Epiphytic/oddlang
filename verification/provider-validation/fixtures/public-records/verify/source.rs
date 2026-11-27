use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Reading {
    pub label: String,
    pub epoch: u64,
    pub flags: Vec<u8>,
    pub active: bool,
}
pub fn unrelated() -> &'static str { "must not enter the projection" }
