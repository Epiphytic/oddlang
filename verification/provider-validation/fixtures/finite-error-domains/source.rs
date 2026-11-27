#[derive(Debug, thiserror::Error)]
pub enum Fault {
    #[error("device unavailable")]
    Device(#[from] std::io::Error),
    #[error("reading: {}", formatter(.0))]
    Reading(sample::Reading),
    #[error("measurement: {0}")]
    Measurement(&'static str),
    #[error("stop")]
    Stop,
}

pub type Outcome<T> = std::result::Result<T, Fault>;

pub(crate) fn formatter(code: &sample::Reading) -> &'static str {
    use sample::Reading::*;
    match code { Missing => "absent", Broken => "unreadable" }
}
