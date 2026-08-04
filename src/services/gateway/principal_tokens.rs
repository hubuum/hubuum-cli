use chrono::{DateTime, Utc};
use hubuum_client::{
    HubuumDateTime, NewTokenRequest, Permissions, PrincipalTokenMetadata, TokenId,
};
use std::str::FromStr;

use crate::domain::IssuedTokenRecord;
use crate::errors::AppError;

#[derive(Debug, Clone)]
pub struct NewTokenInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub expires_at: Option<String>,
    pub scopes: Vec<String>,
}

impl NewTokenInput {
    pub(super) fn into_request(self) -> Result<NewTokenRequest, AppError> {
        let mut request = NewTokenRequest::new();

        if let Some(name) = self.name {
            request = request.name(name);
        }
        if let Some(description) = self.description {
            request = request.description(description);
        }
        if let Some(expires_at) = self.expires_at.as_deref() {
            request = request.expires_at(parse_expiry(expires_at)?);
        }
        if !self.scopes.is_empty() {
            let scopes = self
                .scopes
                .iter()
                .map(|scope| {
                    Permissions::from_str(scope).map_err(|_| {
                        AppError::CommandExecutionError(format!(
                            "unknown permission scope: {scope}"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            request = request.scopes(scopes);
        }

        Ok(request)
    }
}

#[derive(Debug, Clone)]
pub struct CloneTokenInput {
    source_token_id: TokenId,
    name: Option<String>,
    description: Option<String>,
    expires_at: Option<String>,
    revoke_source: bool,
}

impl CloneTokenInput {
    pub fn new(source_token_id: TokenId) -> Self {
        Self {
            source_token_id,
            name: None,
            description: None,
            expires_at: None,
            revoke_source: false,
        }
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn expires_at(mut self, expires_at: Option<String>) -> Self {
        self.expires_at = expires_at;
        self
    }

    pub fn revoke_source(mut self, revoke_source: bool) -> Self {
        self.revoke_source = revoke_source;
        self
    }

    pub fn source_token_id(&self) -> TokenId {
        self.source_token_id
    }

    pub(super) fn should_revoke_source(&self) -> bool {
        self.revoke_source
    }

    pub(super) fn request_for(
        &self,
        source: &PrincipalTokenMetadata,
    ) -> Result<NewTokenRequest, AppError> {
        let mut request = NewTokenRequest::new();

        if let Some(name) = self.name.as_ref().or(source.name.as_ref()) {
            request = request.name(name.clone());
        }
        if let Some(description) = self.description.as_ref().or(source.description.as_ref()) {
            request = request.description(description.clone());
        }
        if let Some(expires_at) = self.expires_at.as_deref() {
            request = request.expires_at(parse_expiry(expires_at)?);
        }
        if let Some(scope) = source.scope.clone() {
            request = request.scope(scope);
        }

        Ok(request)
    }
}

#[derive(Debug)]
pub enum SourceTokenRevocation {
    NotRequested,
    Revoked,
    Failed(String),
}

#[derive(Debug)]
pub struct CloneTokenOutcome {
    issued_token: IssuedTokenRecord,
    source_token_id: TokenId,
    source_revocation: SourceTokenRevocation,
}

impl CloneTokenOutcome {
    pub(super) fn new(
        issued_token: IssuedTokenRecord,
        source_token_id: TokenId,
        source_revocation: SourceTokenRevocation,
    ) -> Self {
        Self {
            issued_token,
            source_token_id,
            source_revocation,
        }
    }

    pub fn issued_token(&self) -> &IssuedTokenRecord {
        &self.issued_token
    }

    pub fn source_token_id(&self) -> TokenId {
        self.source_token_id
    }

    pub fn source_revocation(&self) -> &SourceTokenRevocation {
        &self.source_revocation
    }
}

pub(super) fn find_source_token(
    tokens: impl IntoIterator<Item = PrincipalTokenMetadata>,
    token_id: TokenId,
) -> Result<PrincipalTokenMetadata, AppError> {
    tokens
        .into_iter()
        .find(|token| token.id == token_id)
        .ok_or_else(|| AppError::EntityNotFound(format!("active token {token_id}")))
}

fn parse_expiry(value: &str) -> Result<HubuumDateTime, AppError> {
    let date_time = DateTime::parse_from_rfc3339(value)
        .map_err(|error| {
            AppError::CommandExecutionError(format!(
                "invalid --expires-at (expected RFC3339, e.g. 2026-12-31T23:59:59Z): {error}"
            ))
        })?
        .with_timezone(&Utc);
    Ok(HubuumDateTime(date_time))
}

#[cfg(test)]
mod tests {
    use hubuum_client::PrincipalTokenMetadata;
    use serde_json::{from_value, json, to_value};

    use super::CloneTokenInput;

    #[test]
    fn clone_request_preserves_exact_scope_and_uses_fresh_default_expiry() {
        let source: PrincipalTokenMetadata = from_value(json!({
            "id": 20,
            "principal_id": 7,
            "name": "ansible-facts",
            "description": "Publish host facts",
            "scope": {
                "permissions": ["ReadObject", "UpdateObject"],
                "resources": [
                    {"kind": "collection", "id": 3},
                    {"kind": "class", "id": 8},
                    {"kind": "object", "id": 42}
                ]
            },
            "issued": "2026-07-25T18:43:41Z",
            "expires_at": "2029-01-01T20:42:00Z",
            "last_used_at": null,
            "revoked_at": null
        }))
        .expect("source token should deserialize");

        let request = CloneTokenInput::new(20.into())
            .request_for(&source)
            .expect("clone request should build");
        let value = to_value(request).expect("clone request should serialize");

        assert_eq!(value["name"], "ansible-facts");
        assert_eq!(value["description"], "Publish host facts");
        assert_eq!(
            value["scope"],
            source.scope.map(|scope| to_value(scope).unwrap()).unwrap()
        );
        assert!(value.get("expires_at").is_none());
    }

    #[test]
    fn clone_request_applies_metadata_and_expiry_overrides() {
        let source: PrincipalTokenMetadata = from_value(json!({
            "id": 20,
            "principal_id": 7,
            "name": "old-name",
            "description": "old description",
            "scope": null,
            "issued": "2026-07-25T18:43:41Z",
            "expires_at": "2029-01-01T20:42:00Z",
            "last_used_at": null,
            "revoked_at": null
        }))
        .expect("source token should deserialize");

        let request = CloneTokenInput::new(20.into())
            .name(Some("new-name".to_string()))
            .description(Some("new description".to_string()))
            .expires_at(Some("2030-01-01T00:00:00Z".to_string()))
            .request_for(&source)
            .expect("clone request should build");
        let value = to_value(request).expect("clone request should serialize");

        assert_eq!(value["name"], "new-name");
        assert_eq!(value["description"], "new description");
        assert_eq!(value["expires_at"], "2030-01-01T00:00:00");
        assert!(value.get("scope").is_none());
    }
}
