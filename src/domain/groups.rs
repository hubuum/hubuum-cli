use hubuum_client::Group;
use serde::{Deserialize, Serialize};

use super::UserRecord;

transparent_record!(GroupRecord, Group);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDetails {
    pub group: GroupRecord,
    pub members: Vec<UserRecord>,
}
