use hubuum_client::{
    NewRemoteTarget, RemoteAuthConfig, RemoteHttpMethod, RemoteInvocationSubject,
    RemoteTargetInvokeRequest, RemoteTargetSubjectType, UpdateRemoteTarget,
};

use crate::domain::{RemoteTargetRecord, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    apply_query_paging, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::HubuumGateway;

#[derive(Debug, Clone)]
pub struct CreateRemoteTargetInput {
    pub collection: String,
    pub name: String,
    pub description: String,
    pub method: String,
    pub url_template: String,
    pub allowed_subject_types: Vec<String>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub headers_template: Option<serde_json::Value>,
    pub timeout_ms: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct UpdateRemoteTargetInput {
    pub name: String,
    pub rename: Option<String>,
    pub description: Option<String>,
    pub collection: Option<String>,
    pub method: Option<String>,
    pub url_template: Option<String>,
    pub allowed_subject_types: Option<Vec<String>>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub headers_template: Option<serde_json::Value>,
    pub timeout_ms: Option<i32>,
}

#[derive(Debug, Clone)]
pub enum RemoteAuthConfigInput {
    None,
    BearerSecret { secret: String },
    BasicSecret { username: String, secret: String },
    ApiKeySecret { header: String, secret: String },
}

#[derive(Debug, Clone)]
pub struct InvokeRemoteTargetInput {
    pub subject_kind: String,
    pub collection: Option<String>,
    pub class: Option<String>,
    pub object: Option<String>,
    pub class_a: Option<String>,
    pub class_b: Option<String>,
    pub object_a: Option<String>,
    pub object_b: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub body_override: Option<serde_json::Value>,
}

fn parse_method(method_str: &str) -> Result<RemoteHttpMethod, AppError> {
    match method_str.to_lowercase().as_str() {
        "get" => Ok(RemoteHttpMethod::Get),
        "post" => Ok(RemoteHttpMethod::Post),
        "patch" => Ok(RemoteHttpMethod::Patch),
        "delete" => Ok(RemoteHttpMethod::Delete),
        _ => Err(AppError::ParseError(format!(
            "Invalid HTTP method '{}'. Valid values: get, post, patch, delete",
            method_str
        ))),
    }
}

fn parse_subject_type(type_str: &str) -> Result<RemoteTargetSubjectType, AppError> {
    match type_str.to_lowercase().as_str() {
        "collection" => Ok(RemoteTargetSubjectType::Collection),
        "class" => Ok(RemoteTargetSubjectType::Class),
        "object" => Ok(RemoteTargetSubjectType::Object),
        "class_relation" | "classrelation" => Ok(RemoteTargetSubjectType::ClassRelation),
        "object_relation" | "objectrelation" => Ok(RemoteTargetSubjectType::ObjectRelation),
        _ => Err(AppError::ParseError(format!(
            "Invalid subject type '{}'. Valid values: collection, class, object, class_relation, object_relation",
            type_str
        ))),
    }
}

fn parse_auth_config(input: RemoteAuthConfigInput) -> RemoteAuthConfig {
    match input {
        RemoteAuthConfigInput::None => RemoteAuthConfig::None,
        RemoteAuthConfigInput::BearerSecret { secret } => RemoteAuthConfig::BearerSecret { secret },
        RemoteAuthConfigInput::BasicSecret { username, secret } => {
            RemoteAuthConfig::BasicSecret { username, secret }
        }
        RemoteAuthConfigInput::ApiKeySecret { header, secret } => {
            RemoteAuthConfig::ApiKeySecret { header, secret }
        }
    }
}

fn build_invocation_subject(
    gateway: &HubuumGateway,
    input: &InvokeRemoteTargetInput,
) -> Result<RemoteInvocationSubject, AppError> {
    match input.subject_kind.to_lowercase().as_str() {
        "collection" => {
            let collection_id = gateway.collection_id(input.collection.as_deref().ok_or_else(|| {
                AppError::MissingOptions(vec!["collection".to_string()])
            })?)?;
            Ok(RemoteInvocationSubject::Collection { collection_id })
        }
        "class" => {
            let class_id = gateway
                .class_handle_by_name(input.class.as_deref().ok_or_else(|| {
                    AppError::MissingOptions(vec!["class".to_string()])
                })?)?
                .id().into();
            Ok(RemoteInvocationSubject::Class { class_id })
        }
        "object" => {
            let class = input
                .class
                .as_deref()
                .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
            let object = gateway.object_handle_by_name(
                class,
                input
                    .object
                    .as_deref()
                    .ok_or_else(|| AppError::MissingOptions(vec!["object".to_string()]))?,
            )?;
            let class_id = object.resource().hubuum_class_id;
            let object_id = object.id().into();
            Ok(RemoteInvocationSubject::Object { class_id, object_id })
        }
        "class_relation" | "classrelation" => {
            let relation = gateway.get_class_relation_by_pair(
                input
                    .class_a
                    .as_deref()
                    .ok_or_else(|| AppError::MissingOptions(vec!["class-a".to_string()]))?,
                input
                    .class_b
                    .as_deref()
                    .ok_or_else(|| AppError::MissingOptions(vec!["class-b".to_string()]))?,
            )?;
            let relation_id = relation.id;
            Ok(RemoteInvocationSubject::ClassRelation { relation_id })
        }
        "object_relation" | "objectrelation" => {
            let relation = gateway.get_object_relation_v2(&crate::services::RelationTarget {
                class_a: input
                    .class_a
                    .clone()
                    .ok_or_else(|| AppError::MissingOptions(vec!["class-a".to_string()]))?,
                object_a: Some(input
                    .object_a
                    .clone()
                    .ok_or_else(|| AppError::MissingOptions(vec!["object-a".to_string()]))?),
                class_b: input
                    .class_b
                    .clone()
                    .ok_or_else(|| AppError::MissingOptions(vec!["class-b".to_string()]))?,
                object_b: Some(input
                    .object_b
                    .clone()
                    .ok_or_else(|| AppError::MissingOptions(vec!["object-b".to_string()]))?),
            })?;
            let relation_id = relation.id;
            Ok(RemoteInvocationSubject::ObjectRelation { relation_id })
        }
        _ => Err(AppError::ParseError(format!(
            "Invalid subject kind '{}'. Valid values: collection, class, object, class_relation, object_relation",
            input.subject_kind
        ))),
    }
}

impl HubuumGateway {
    pub fn list_remote_target_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_remote_targets(&ListQuery {
                limit: Some(200),
                ..ListQuery::default()
            })?
            .items
            .into_iter()
            .map(|target| target.0.name)
            .collect())
    }

    pub fn create_remote_target(
        &self,
        input: CreateRemoteTargetInput,
    ) -> Result<RemoteTargetRecord, AppError> {
        let method = parse_method(&input.method)?;
        let allowed_subject_types = input
            .allowed_subject_types
            .iter()
            .map(|s| parse_subject_type(s))
            .collect::<Result<Vec<_>, _>>()?;

        let new_target = NewRemoteTarget {
            collection_id: self.collection_id(&input.collection)?,
            name: input.name,
            description: input.description,
            method,
            url_template: input.url_template,
            allowed_subject_types,
            auth_config: input.auth_config.map(parse_auth_config),
            body_template: input.body_template,
            class_id: input
                .class
                .as_deref()
                .map(|class| {
                    self.class_handle_by_name(class)
                        .map(|handle| handle.id().into())
                })
                .transpose()?,
            enabled: input.enabled,
            headers_template: input.headers_template,
            timeout_ms: input.timeout_ms,
        };

        let target = self
            .client
            .remote_targets()
            .create()
            .params(new_target)
            .send()?;
        Ok(RemoteTargetRecord::from(target))
    }

    pub fn list_remote_targets(
        &self,
        query: &ListQuery,
    ) -> Result<PagedResult<RemoteTargetRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, REMOTE_TARGET_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, REMOTE_TARGET_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;

        let page = apply_query_paging(
            self.client.remote_targets().query().filters(filters),
            query,
            &validated_sorts,
        )
        .page()?;
        Ok(PagedResult::from_page(
            page,
            query.limit,
            RemoteTargetRecord::from,
        ))
    }

    pub fn remote_target(&self, name: &str) -> Result<RemoteTargetRecord, AppError> {
        let target = self.client.remote_targets().get_by_name(name)?;
        Ok(RemoteTargetRecord::from(target.resource()))
    }

    pub fn update_remote_target(
        &self,
        input: UpdateRemoteTargetInput,
    ) -> Result<RemoteTargetRecord, AppError> {
        let target = self.client.remote_targets().get_by_name(&input.name)?;

        let method = input.method.as_ref().map(|m| parse_method(m)).transpose()?;
        let allowed_subject_types = input
            .allowed_subject_types
            .as_ref()
            .map(|types| {
                types
                    .iter()
                    .map(|s| parse_subject_type(s))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;

        let update = UpdateRemoteTarget {
            name: input.rename,
            description: input.description,
            collection_id: input
                .collection
                .as_deref()
                .map(|collection| self.collection_id(collection))
                .transpose()?,
            method,
            url_template: input.url_template,
            headers_template: input.headers_template,
            auth_config: input.auth_config.map(parse_auth_config),
            allowed_subject_types,
            body_template: input.body_template,
            class_id: input
                .class
                .as_deref()
                .map(|class| {
                    self.class_handle_by_name(class)
                        .map(|handle| handle.id().into())
                })
                .transpose()?,
            enabled: input.enabled,
            timeout_ms: input.timeout_ms,
        };

        let updated = self
            .client
            .remote_targets()
            .update(target.id())
            .params(update)
            .send()?;
        Ok(RemoteTargetRecord::from(updated))
    }

    pub fn delete_remote_target(&self, name: &str) -> Result<(), AppError> {
        let target = self.client.remote_targets().get_by_name(name)?;
        self.client.remote_targets().delete(target.id())?;
        Ok(())
    }

    pub fn invoke_remote_target(
        &self,
        name: &str,
        input: InvokeRemoteTargetInput,
    ) -> Result<TaskRecord, AppError> {
        let handle = self.client.remote_targets().get_by_name(name)?;
        let subject = build_invocation_subject(self, &input)?;
        let mut req = RemoteTargetInvokeRequest::new(subject);
        if let Some(p) = input.parameters {
            req = req.parameters(p);
        }
        if let Some(b) = input.body_override {
            req = req.body_override(b);
        }
        Ok(TaskRecord(handle.invoke(req)?))
    }
}

pub(crate) const REMOTE_TARGET_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "collection_id",
        "collection_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "enabled",
        "enabled",
        FilterOperatorProfile::String,
        FilterValueProfile::Boolean,
    ),
];

pub(crate) const REMOTE_TARGET_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("collection_id", "collection_id"),
    SortFieldSpec::new("enabled", "enabled"),
];
