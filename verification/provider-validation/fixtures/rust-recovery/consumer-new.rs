pub struct Held; mod Hidden {} pub const Meter: u8 = 3;
                #[macro_export] macro_rules! Mac { () => { 2 }; }
                mod a { pub struct Clash; } mod b { pub struct Clash; } pub use a::*; pub use b::*;
#[cfg(local_profile)] pub struct LocalType;