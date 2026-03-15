use hubuum_client::Class;
use serde::{Deserialize, Serialize};

use super::{ObjectRecord, RelatedClassTreeNode};

transparent_record!(ClassRecord, Class);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassShowRecord {
    pub class: ClassRecord,
    pub objects: Vec<ObjectRecord>,
    pub related_classes: Vec<RelatedClassTreeNode>,
}
