use crate::diagnostics::client_call;
use crate::domain::{
    GroupRecord, IssuedTokenRecord, MeRecord, PrincipalPermissionsRecord, PrincipalTokenRecord,
};
use crate::errors::AppError;

use super::{HubuumGateway, NewTokenInput};

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

impl HubuumGateway {
    /// Resolve the authenticated principal, never an ambiguous display name.
    pub(crate) fn current_token_create(
        &self,
        input: NewTokenInput,
    ) -> Result<IssuedTokenRecord, AppError> {
        let client = self.client();
        let principal = client_call("client.me", || client.me())?
            .principal
            .principal_id;
        let token = self.create_approved_token(&client, principal, input.into_request()?)?;
        Ok(token.into())
    }
}
