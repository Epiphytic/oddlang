
mod hidden {
    pub struct Reading { pub units: u16, pub(crate) calibration: i8, internal: bool }
    #[non_exhaustive] pub struct Incomplete { pub public_field: bool }
    pub enum Payload { Unit, Tuple(u64), Named { item: bool } }
    pub union Bits { pub first: u32, hidden: u8 }
    pub struct Tuple(pub i16, i8);
}
pub use hidden::{Reading as Report, Tuple, Payload, Bits, Incomplete};
pub use hidden::Payload::Named;
pub type ReadingAlias = Report;
pub use std::num::NonZeroU8 as External;
pub use std::fmt::Debug as _;
pub mod outward { pub use super::routes as nested; }
pub use routes as mirror;
pub mod routes {
    pub use super::Report as Alias;
    pub use crate as again;
}
pub mod glob { pub use super::routes::*; }
mod left { pub struct Conflict { pub from_left: u8 } }
mod right { pub struct Conflict { pub from_right: u8 } }
pub mod mixed { pub use super::left::*; pub use super::right::*; }
#[cfg(feature = "extra")]
pub use hidden::Reading as Extra;
