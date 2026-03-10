use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub hostname: String,
    pub username: String,
    pub token: String,
}
