use hubuum_client::{
    CredentialActivityPageOptions, CredentialActivityResponse, PrincipalId, TokenId,
};

use super::HubuumGateway;
use crate::diagnostics::client_call;
use crate::errors::AppError;

pub enum CredentialOwner {
    Current,
    User(String),
    ServiceAccount(String),
}

pub struct CredentialActivityInput {
    token_id: TokenId,
    page: CredentialActivityPageOptions,
    all: bool,
}

impl CredentialActivityInput {
    pub fn new(token_id: TokenId, page: CredentialActivityPageOptions) -> Result<Self, AppError> {
        if token_id.get() <= 0 {
            return Err(AppError::InvalidOption(
                "--token-id must be positive".into(),
            ));
        }
        Ok(Self {
            token_id,
            page,
            all: false,
        })
    }
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }
}

impl HubuumGateway {
    pub fn credential_activity(
        &self,
        owner: CredentialOwner,
        input: CredentialActivityInput,
    ) -> Result<CredentialActivityResponse, AppError> {
        let client = self.client();
        let principal: PrincipalId = match owner {
            CredentialOwner::Current => {
                client_call("client.me", || client.me())?
                    .principal
                    .principal_id
            }
            CredentialOwner::User(name) => {
                client_call("users.get_by_name", || client.users().get_by_name(&name))?
                    .id()
                    .into()
            }
            CredentialOwner::ServiceAccount(name) => {
                client_call("service_accounts.get_by_name", || {
                    client.service_accounts().get_by_name(&name)
                })?
                .id()
                .into()
            }
        };
        let mut result = client_call("client.token_activity", || {
            client.token_activity(principal, input.token_id, input.page)
        })?;
        validate_activity_page(&result, input.page)?;
        let mut next_options = result.next_page_options()?;
        if !input.all {
            return Ok(result);
        }
        while let Some(options) = next_options {
            let mut next = client_call("client.token_activity", || {
                client.token_activity(principal, input.token_id, options)
            })?;
            validate_activity_page(&next, options)?;
            next_options = next.next_page_options()?;
            if result.session_id != next.session_id
                || result.creation != next.creation
                || result.retention_days != next.retention_days
                || result.source_cap != next.source_cap
                || result.collection_enabled != next.collection_enabled
                || result.total != next.total
            {
                return Err(AppError::InvalidOption(
                    "Credential activity changed between pages; restart the query".into(),
                ));
            }
            result.sources.append(&mut next.sources);
            result.next_offset = next.next_offset;
            result.overflow_since = next.overflow_since;
            result.best_effort |= next.best_effort;
        }
        Ok(result)
    }
}

fn validate_activity_page(
    page: &CredentialActivityResponse,
    options: CredentialActivityPageOptions,
) -> Result<(), AppError> {
    if page.offset != options.offset_value() || page.sources.len() > page.limit {
        return Err(AppError::InvalidOption(
            "Credential activity returned inconsistent pagination; restart the query".into(),
        ));
    }
    Ok(())
}
