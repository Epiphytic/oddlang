use serde::{Serialize, Deserialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mark { Dry, Damp }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plot {
    pub from: u32,
    pub until: Option<u32>,
    pub mark: Option<Mark>,
    pub fixed: bool,
}
