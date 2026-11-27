pub struct Held; pub struct Gone; pub struct Hidden; pub struct Meter; pub struct Mac; pub struct Clash; mod marker {pub trait Tag {}} pub use marker::Tag as _;
                pub mod removed { pub struct Leaf; }
#[cfg(local_profile)] pub struct LocalType;