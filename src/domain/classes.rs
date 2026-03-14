use hubuum_client::Class;
use serde::{Deserialize, Serialize};

use super::ObjectRecord;

transparent_record!(ClassRecord, Class);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDetails {
    pub class: ClassRecord,
    pub objects: Vec<ObjectRecord>,
}
