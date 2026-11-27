use serde::{Serialize,Deserialize};
#[derive(Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample { pub seq:u16, #[serde(with="base64_sample")] pub signature:Vec<u8> }
