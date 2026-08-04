use hubuum_client::TokenId;

use crate::domain::{
    IssuedTokenRecord, PrincipalTokenDetailsRecord, PrincipalTokenRecord, ServiceAccountRecord,
};
use crate::errors::AppError;
use crate::list_query::{
    fetch_query_results, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::{
    principal_tokens::find_source_token, CloneTokenInput, CloneTokenOutcome, HubuumGateway,
    NewTokenInput, SourceTokenRevocation,
};

#[derive(Debug, Clone)]
pub struct CreateServiceAccountInput {
    pub name: String,
    pub description: Option<String>,
    pub owner_group_id: i32,
}

impl HubuumGateway {
    pub fn list_service_account_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_service_accounts(&ListQuery {
                limit: Some(200),
                ..ListQuery::default()
            })?
            .items
            .into_iter()
            .map(|account| account.0.name)
            .collect())
    }

    pub fn create_service_account(
        &self,
        input: CreateServiceAccountInput,
    ) -> Result<ServiceAccountRecord, AppError> {
        let mut create = self
            .client
            .service_accounts()
            .create_checked()
            .name(input.name)
            .owner_group_id(input.owner_group_id);
        if let Some(description) = input.description {
            create = create.description(description);
        }
        let sa = create.send()?;

        Ok(ServiceAccountRecord::from(sa))
    }

    pub fn list_service_accounts(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<ServiceAccountRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, SERVICE_ACCOUNT_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, SERVICE_ACCOUNT_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let mut query_op = self.client.service_accounts().query();
        for filter in filters {
            query_op = query_op.filter(&filter.key, filter.operator, &filter.value);
        }

        let page = fetch_query_results(query_op, query, &validated_sorts)?;
        Ok(page.map(ServiceAccountRecord::from))
    }

    pub fn service_account(&self, name: &str) -> Result<ServiceAccountRecord, AppError> {
        let sa = self.client.service_accounts().get_by_name(name)?;
        Ok(ServiceAccountRecord::from(sa.resource().clone()))
    }

    pub fn service_account_id_by_name(&self, name: &str) -> Result<i32, AppError> {
        Ok(self
            .client
            .service_accounts()
            .get_by_name(name)?
            .id()
            .into())
    }

    pub fn delete_service_account(&self, name: &str) -> Result<(), AppError> {
        let sa = self.client.service_accounts().get_by_name(name)?;
        self.client.service_accounts().delete(sa.id())?;
        Ok(())
    }

    pub fn disable_service_account(&self, name: &str) -> Result<ServiceAccountRecord, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        let disabled = handle.disable()?;
        Ok(ServiceAccountRecord::from(disabled))
    }

    pub fn service_account_tokens(
        &self,
        name: &str,
    ) -> Result<Vec<PrincipalTokenRecord>, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        let tokens = handle.tokens()?;
        Ok(tokens.into_iter().map(PrincipalTokenRecord::from).collect())
    }

    pub fn list_service_account_token_ids(&self, name: &str) -> Result<Vec<String>, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        Ok(handle
            .tokens()?
            .into_iter()
            .map(|token| token.id.to_string())
            .collect())
    }

    pub fn service_account_token(
        &self,
        name: &str,
        token_id: TokenId,
    ) -> Result<PrincipalTokenDetailsRecord, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        self.principal_token_details(handle.tokens()?, token_id)
    }

    pub fn service_account_token_create(
        &self,
        name: &str,
        input: NewTokenInput,
    ) -> Result<IssuedTokenRecord, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        Ok(handle.tokens_create_token(input.into_request()?)?.into())
    }

    pub fn service_account_token_clone(
        &self,
        name: &str,
        input: CloneTokenInput,
    ) -> Result<CloneTokenOutcome, AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        let source_token_id = input.source_token_id();
        let source = find_source_token(handle.tokens()?, source_token_id)?;
        let issued_token = handle
            .tokens_create_token(input.request_for(&source)?)?
            .into();
        let source_revocation = if input.should_revoke_source() {
            match handle.token_revoke(source_token_id) {
                Ok(()) => SourceTokenRevocation::Revoked,
                Err(error) => SourceTokenRevocation::Failed(error.to_string()),
            }
        } else {
            SourceTokenRevocation::NotRequested
        };

        Ok(CloneTokenOutcome::new(
            issued_token,
            source_token_id,
            source_revocation,
        ))
    }

    pub fn service_account_token_revoke(&self, name: &str, token_id: i32) -> Result<(), AppError> {
        let handle = self.client.service_accounts().get_by_name(name)?;
        handle.token_revoke(token_id)?;
        Ok(())
    }
}

pub(crate) const SERVICE_ACCOUNT_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "owner_group_id",
        "owner_group_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
];

pub(crate) const SERVICE_ACCOUNT_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("owner_group_id", "owner_group_id"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{Method, StatusCode};
    use serde_json::{from_slice, json, Value};

    use super::{CloneTokenInput, HubuumGateway, SourceTokenRevocation};

    #[test]
    fn clone_creates_replacement_before_revoking_source() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([{
                    "id": 7,
                    "name": "mi-ansible-facts",
                    "description": "",
                    "owner_group_id": 2,
                    "created_by": 1,
                    "disabled_at": null,
                    "created_at": "2026-07-25T12:00:00Z",
                    "updated_at": "2026-07-25T12:00:00Z"
                }]),
            )
            .expect("service-account response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!([{
                    "id": 20,
                    "principal_id": 7,
                    "name": "ansible-facts",
                    "description": "Publish host facts",
                    "scope": {
                        "permissions": ["ReadObject", "UpdateObject"],
                        "resources": [{"kind": "class", "id": 8}]
                    },
                    "issued": "2026-07-25T18:43:41Z",
                    "expires_at": "2029-01-01T20:42:00Z",
                    "last_used_at": null,
                    "revoked_at": null
                }]),
            )
            .expect("token-list response should serialize"),
        );
        transport.push_response(
            TransportResponse::json(
                StatusCode::CREATED,
                &json!({
                    "token": "replacement-secret",
                    "expires_at": "2026-08-05T00:00:00Z"
                }),
            )
            .expect("token-create response should serialize"),
        );
        transport.push_response(TransportResponse::empty(StatusCode::NO_CONTENT));
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let outcome = gateway
            .service_account_token_clone(
                "mi-ansible-facts",
                CloneTokenInput::new(20.into()).revoke_source(true),
            )
            .expect("token clone should succeed");

        assert_eq!(outcome.issued_token().token(), "replacement-secret");
        assert!(matches!(
            outcome.source_revocation(),
            SourceTokenRevocation::Revoked
        ));
        let requests = transport.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2].method, Method::POST);
        assert_eq!(requests[2].url.path(), "/api/v1/iam/principals/7/tokens");
        let body: Value = from_slice(requests[2].body()).expect("request body should be JSON");
        assert_eq!(body["name"], "ansible-facts");
        assert_eq!(body["description"], "Publish host facts");
        assert_eq!(
            body["scope"],
            json!({
                "permissions": ["ReadObject", "UpdateObject"],
                "resources": [{"kind": "class", "id": 8}]
            })
        );
        assert!(body.get("expires_at").is_none());
        assert_eq!(requests[3].method, Method::POST);
        assert_eq!(
            requests[3].url.path(),
            "/api/v1/iam/principals/7/tokens/20/revoke"
        );
    }
}
