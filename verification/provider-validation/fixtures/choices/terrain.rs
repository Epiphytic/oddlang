use serde::{Deserialize, Serialize as WireSerialize};
#[derive(Debug, Clone, PartialEq, Eq, WireSerialize, Deserialize)]
pub enum Terrain {
    #[serde(rename = "wet-ground")]
    Marsh,
    Granite,
}
