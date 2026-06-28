use hubuum_client::{Group, PrincipalMember};
use serde::{Deserialize, Serialize};

transparent_record!(GroupRecord, Group);
transparent_record!(PrincipalMemberRecord, PrincipalMember);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDetails {
    pub group: GroupRecord,
    pub members: Vec<PrincipalMemberRecord>,
}
