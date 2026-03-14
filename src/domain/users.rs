use hubuum_client::User;
use serde::{Deserialize, Serialize};

transparent_record!(UserRecord, User);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedUser {
    pub user: UserRecord,
    pub password: String,
}
