
#![deny(ambiguous_import_visibilities)]
pub mod museum {
    mod catalog { pub struct Entry; }
    pub use catalog::*;
    macro_rules! shadow { () => { use catalog::Entry; } }
    shadow!();
    pub use Entry as Renamed;
}
