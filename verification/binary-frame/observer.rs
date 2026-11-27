#![deny(warnings)]
mod frame;
mod record;
mod bound;
pub use record::Header;
#[derive(Clone, Eq, PartialEq)]
pub struct Existing { quantity:u16, sample:Vec<u8> }
impl Existing { pub fn validate(&self) -> Result<(),Fault> { Ok(()) } }
use frame::{Message,Messages};
#[derive(Debug,PartialEq,Eq)]
pub enum Fault { Bad(&'static str) }
fn main() {
    let seed=Message::Panel(Header{unit:1});
    let bytes=seed.encode().unwrap();
    assert_eq!(bytes,[22,0,0,0,2,0,1]);
    for split in 0..bytes.len() {
        let mut decoder=Messages::default();
        let (used,frame)=decoder.feed(&bytes[..split]).unwrap();
        assert_eq!(used,split);
        assert!(frame.is_none());
        let (used,frame)=decoder.feed(&[&bytes[split..], &bytes].concat()).unwrap();
        assert_eq!(used,bytes.len()-split);
        assert!(frame==Some(seed.clone()));
        assert_eq!(decoder.finish(),Ok(()));
    }
    assert_eq!(Message::decode(&[22,0,0,0,3,0,0,1]).err(),Some(Fault::Bad("unit")));
    assert_eq!(Message::decode(&[25,0,0,0,7,0,0,0,0,0,1,7]).err(),Some(Fault::Bad("unit")));
    assert_eq!(Message::decode(&[25,0,0,0,8,0,1,0,0,0,2,7,8]).err(),Some(Fault::Bad("limit")));
    let valid=Message::Existing(Existing{quantity:1,sample:vec![7]});
    assert!(Message::decode(&valid.encode().unwrap()).unwrap()==valid);
    assert_eq!(Message::decode(&[24,0,0,0,2,0,1]).err(),Some(Fault::Bad("truncated")));
    assert_eq!(Message::decode(&[24,0,0,0,3,0,1,1]).err(),Some(Fault::Bad("stamp")));
    assert_eq!(Message::Samples{index:13,data:vec![]}.encode().err(),Some(Fault::Bad("data")));
    assert_eq!(Message::decode(&[23,0,0,0,8,0,0,0,0,0,0,0,13]).err(),Some(Fault::Bad("range")));
    assert_eq!(Message::decode(&[0,0,0,0,17]).err(),Some(Fault::Bad("limit")));
    let mut decoder=Messages::default();
    assert_eq!(decoder.feed(&[22,0,0,0,17]).err(),Some(Fault::Bad("limit")));
    assert_eq!(decoder.feed(&[]).err(),Some(Fault::Bad("decoderFailed")));
    assert_eq!(decoder.finish(),Err(Fault::Bad("truncated")));
}
