
struct Lookahead<T> {
    feed: T,
    octets: zeroize::Zeroizing<[u8; 7]>,
    used: usize,
    loaded: usize,
}
