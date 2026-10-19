use crate::diagnostics::client_call;
use crate::domain::{GroupRecord, MeRecord, PrincipalPermissionsRecord, PrincipalTokenRecord};
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn me(&self) -> Result<MeRecord, AppError> {
        Ok(MeRecord(client_call("client.me", || self.client().me())?))
    }

    pub fn me_groups(&self) -> Result<Vec<GroupRecord>, AppError> {
        Ok(
            client_call("client.me_groups", || self.client().me_groups())?
                .into_iter()
                .map(|h| GroupRecord::from(h.resource().clone()))
                .collect(),
        )
    }

    pub fn me_tokens(&self) -> Result<Vec<PrincipalTokenRecord>, AppError> {
        Ok(
            client_call("client.me_tokens", || self.client().me_tokens())?
                .into_iter()
                .map(PrincipalTokenRecord::from)
                .collect(),
        )
    }

    pub fn me_permissions(&self) -> Result<Vec<PrincipalPermissionsRecord>, AppError> {
        Ok(
            client_call("client.me_permissions", || self.client().me_permissions())?
                .into_iter()
                .map(PrincipalPermissionsRecord::from)
                .collect(),
        )
    }
}
