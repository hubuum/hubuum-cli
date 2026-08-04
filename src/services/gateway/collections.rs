use hubuum_client::{CollectionPatch, CollectionPost};

use crate::domain::{
    CollectionPermission, CollectionPermissionsView, CollectionRecord, GroupPermissionsRecord,
    GroupPermissionsSummary,
};
use crate::errors::AppError;
use crate::list_query::{
    fetch_query_results, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateCollectionInput {
    pub name: String,
    pub description: String,
    pub owner: String,
}

#[derive(Debug, Clone)]
pub struct CollectionUpdateInput {
    pub name: String,
    pub rename: Option<String>,
    pub description: Option<String>,
}

impl HubuumGateway {
    pub fn list_collection_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .collections()
            .query()
            .list()?
            .into_iter()
            .map(|collection| collection.name)
            .collect())
    }

    pub fn create_collection(
        &self,
        input: CreateCollectionInput,
    ) -> Result<CollectionRecord, AppError> {
        let group = self.client.groups().get_by_name(&input.owner)?;
        let collection = self.client.collections().create_raw(CollectionPost {
            name: input.name,
            description: input.description,
            group_id: group.id(),
            parent_collection_id: None,
        })?;
        Ok(CollectionRecord::from(collection))
    }

    pub fn list_collections(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<CollectionRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, COLLECTION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, COLLECTION_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = fetch_query_results(
            self.client.collections().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        Ok(page.map(CollectionRecord::from))
    }

    pub fn get_collection(&self, name: &str) -> Result<CollectionRecord, AppError> {
        let collection = self.client.collections().get_by_name(name)?;
        Ok(CollectionRecord::from(collection.resource()))
    }

    pub fn delete_collection(&self, name: &str) -> Result<(), AppError> {
        let collection = self.client.collections().get_by_name(name)?;
        self.client.collections().delete(collection.id())?;
        Ok(())
    }

    pub fn update_collection(
        &self,
        input: CollectionUpdateInput,
    ) -> Result<CollectionRecord, AppError> {
        let collection = self.client.collections().get_by_name(&input.name)?;
        let updated = self.client.collections().update_raw(
            collection.id(),
            CollectionPatch {
                name: input.rename,
                description: input.description,
            },
        )?;

        Ok(CollectionRecord::from(updated))
    }

    pub fn list_collection_permissions(
        &self,
        name: &str,
    ) -> Result<CollectionPermissionsView, AppError> {
        let permissions = self.client.collections().get_by_name(name)?.permissions()?;
        let entries = permissions
            .iter()
            .cloned()
            .map(GroupPermissionsRecord::from)
            .collect::<Vec<_>>();
        let summary = permissions
            .into_iter()
            .map(GroupPermissionsSummary::from)
            .collect::<Vec<_>>();

        Ok(CollectionPermissionsView { entries, summary })
    }

    pub fn grant_collection_permissions(
        &self,
        collection_name: &str,
        group_name: &str,
        permissions: &[CollectionPermission],
    ) -> Result<(), AppError> {
        let collection = self.client.collections().get_by_name(collection_name)?;
        let group = self.client.groups().get_by_name(group_name)?;
        collection.grant_permissions(
            group.id(),
            permissions
                .iter()
                .map(|permission| permission.api_name())
                .collect(),
        )?;
        Ok(())
    }

    pub fn principal_collection_permissions(
        &self,
        collection: &str,
        principal_id: i32,
    ) -> Result<Vec<GroupPermissionsRecord>, AppError> {
        let collection = self.client.collections().get_by_name(collection)?;
        Ok(collection
            .principal_permissions(principal_id)?
            .into_iter()
            .map(GroupPermissionsRecord::from)
            .collect())
    }
}

pub(crate) const COLLECTION_FILTER_SPECS: &[FilterFieldSpec] = &[
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

pub(crate) const COLLECTION_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("description", "description"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{
        header::{HeaderName, HeaderValue},
        StatusCode,
    };
    use serde_json::json;

    use super::HubuumGateway;
    use crate::list_query::{ListQuery, PageSelection};

    #[test]
    fn list_collections_fetches_every_page_when_requested() {
        let transport = MockTransport::default();
        let mut first_page =
            TransportResponse::json(StatusCode::OK, &json!([collection_json(1, "First")]))
                .expect("first page should serialize");
        first_page.headers.insert(
            HeaderName::from_static("x-next-cursor"),
            HeaderValue::from_static("page-2"),
        );
        transport.push_response(first_page);
        transport.push_response(
            TransportResponse::json(StatusCode::OK, &json!([collection_json(2, "Second")]))
                .expect("second page should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let results = gateway
            .list_collections(&ListQuery {
                include_total: true,
                page_selection: PageSelection::All,
                ..ListQuery::default()
            })
            .expect("all collection pages should load");

        assert_eq!(
            results
                .items
                .iter()
                .map(|collection| collection.0.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert_eq!(results.returned_count, 2);
        assert_eq!(results.total_count, Some(2));
        assert!(results.next_cursor.is_none());
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .url
            .query_pairs()
            .any(|(key, value)| key == "cursor" && value == "page-2"));
    }

    fn collection_json(id: i32, name: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": name,
            "description": "",
            "parent_collection_id": null,
            "created_at": "2026-07-25T12:00:00Z",
            "updated_at": "2026-07-25T12:00:00Z"
        })
    }
}
