use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sky { Clear }

#[derive(Clone, Serialize, Deserialize)]
pub struct Forecast { pub sky: Sky }
