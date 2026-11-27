
#[derive(Debug)]
pub enum Fault { Header, Length, Limit, Tag, Truncated, Trailing, DecoderFailed }
