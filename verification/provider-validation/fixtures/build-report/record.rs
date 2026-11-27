
use serde::Serialize;
#[derive(Serialize)]
pub(crate) struct Identification {
    #[serde(rename = "storage-ids")]
    pub stages: [u16; 3],
    pub origin: &'static str,
    pub envelope: u8,
    pub platform: &'static str,
    pub software: &'static str,
}
pub(crate) fn identification() -> Identification {
    Identification {
        envelope: 9,
        software: env!("CARGO_PKG_VERSION"),
        origin: env!("ROVER_SOURCE"),
        stages: [library::PUBLIC as u16, u16::from(crate::format::EARLIEST), crate::format::LATEST],
        platform: env!("ROVER_MACHINE"),
    }
}
