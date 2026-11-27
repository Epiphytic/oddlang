
#![allow(dead_code, unexpected_cfgs)]
macro_rules! declare { () => { fn generated() -> u8 { 2 } }; }
declare!();

mod helpers {
    pub fn first() -> u8 { 1 }
    pub fn second() -> u8 { 1 }
    pub fn a(n: u8) -> u8 { n }
    pub fn b(n: u8) -> u8 { n }
    fn private() -> u8 { 9 }
}
pub use helpers::first as alias;

pub trait Source { fn get(&self) -> u8; }
struct Item { part: u8 }
struct Outer { item: Item }
struct OnDrop;
impl Drop for OnDrop {
    fn drop(&mut self) { std::hint::black_box(0); }
}

fn scope() { let _held = OnDrop; }
fn nested(value: &Outer) -> u8 { value.item.part }
fn inline_const() -> u8 { const { 4 } }
enum Discriminant { Variant = 5 }
fn pick(values: Vec<u8>) -> Vec<u8> {
    values.into_iter().map(helpers::a).collect()
}

#[cfg(any(unpinned_rustflag, unpinned_encoded, debug_assertions))]
fn extra_unpinned() {}
impl Source for Item {
    fn get(&self) -> u8 { self.part }
}

fn private(value: &mut Item) -> u8 {
    value.part = helpers::first() + renamed::combine(generated());
    value.part
}

#[cfg(feature = "extra")]
pub fn dynamic(value: &dyn Source) -> u8 { value.get() }

fn unused_private() -> u8 { 3 }

pub fn concrete() -> u8 {
    let mut item = Item { part: 0 };
    private(&mut item)
}
