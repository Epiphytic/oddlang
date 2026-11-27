
mod helper;
use serde::Serialize;
#[derive(Serialize)]
struct Identification { software: &'static str }
fn identification() -> Identification {
    Identification { software: env!("CARGO_PKG_VERSION") }
}
