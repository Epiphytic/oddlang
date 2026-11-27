
#![allow(dead_code)]
use std::sync::Arc;

struct Frame<'a, T> {
    word: u16,
    shared: Arc<T>,
    map: Choice<T>,
    read: &'a str,
    write: &'a mut T,
    mixed: (i8, bool),
    array: [u8; 3],
    #[cfg(feature = "expanded")]
    extra: f32,
}
enum Choice<T> {
    Zero,
    One(T, u128),
    More { raw: *const T },
    EmptyTuple(),
    EmptyRecord {},
}
struct Packet(u32, i64);
struct Marker;
union Bits { left: u32, right: i32 }
