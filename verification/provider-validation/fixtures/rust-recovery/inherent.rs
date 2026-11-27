
pub mod sensor {
    pub struct Meter;
    impl Meter { pub const LIMIT: u8 = 3; pub fn read(&self) {} fn hidden() {} }
    #[doc(hidden)] impl Meter { #[doc(hidden)] pub fn hidden_from_docs() {} }
    #[cfg(not(doc))] impl Meter { pub fn compiler_only() {} }
    pub trait Measure { fn derived(); }
    impl Measure for Meter { fn derived() {} }
    pub struct Batch<T>(pub T);
    impl Batch<u8> { pub fn specific(&self) {} }
}
