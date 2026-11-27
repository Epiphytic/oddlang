
#![allow(dead_code)]
struct Arc<T>(T);
struct Window<'a, T, const N: usize>(&'a T, [T; N]);
struct Route<'a> {
    alias: Arc<u8>,
    cache: std::rc::Rc<u32>,
    next: Box<Option<Route<'a>>>,
    bytes: Window<'a, u16, 8>,
    endpoint: &'static str,
    offset: usize,
}
macro_rules! status { () => { enum Status { Ok = 1, Offline } }; }
status!();
