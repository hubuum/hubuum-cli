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
    pub namespace_id: i32,
    pub name: String,
    pub description: String,
    pub method: String,
    pub url_template: String,
    pub allowed_subject_types: Vec<String>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class_id: Option<i32>,
    pub enabled: Option<bool>,
    pub headers_template: Option<serde_json::Value>,
    pub timeout_ms: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct UpdateRemoteTargetInput {
    pub name: String,
    pub rename: Option<String>,
    pub description: Option<String>,
    pub namespace_id: Option<i32>,
    pub method: Option<String>,
    pub url_template: Option<String>,
    pub allowed_subject_types: Option<Vec<String>>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class_id: Option<i32>,
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
    pub namespace_id: Option<i32>,
    pub class_id: Option<i32>,
    pub object_id: Option<i32>,
    pub relation_id: Option<i32>,
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
        "namespace" => Ok(RemoteTargetSubjectType::Namespace),
        "class" => Ok(RemoteTargetSubjectType::Class),
        "object" => Ok(RemoteTargetSubjectType::Object),
        "class_relation" | "classrelation" => Ok(RemoteTargetSubjectType::ClassRelation),
        "object_relation" | "objectrelation" => Ok(RemoteTargetSubjectType::ObjectRelation),
        _ => Err(AppError::ParseError(format!(
            "Invalid subject type '{}'. Valid values: namespace, class, object, class_relation, object_relation",
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
    input: &InvokeRemoteTargetInput,
) -> Result<RemoteInvocationSubject, AppError> {
    match input.subject_kind.to_lowercase().as_str() {
        "namespace" => {
            let namespace_id = input.namespace_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["namespace-id".to_string()])
            })?;
            Ok(RemoteInvocationSubject::Namespace { namespace_id })
        }
        "class" => {
            let class_id = input.class_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["class-id".to_string()])
            })?;
            Ok(RemoteInvocationSubject::Class { class_id })
        }
        "object" => {
            let class_id = input.class_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["class-id".to_string()])
            })?;
            let object_id = input.object_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["object-id".to_string()])
            })?;
            Ok(RemoteInvocationSubject::Object { class_id, object_id })
        }
        "class_relation" | "classrelation" => {
            let relation_id = input.relation_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["relation-id".to_string()])
            })?;
            Ok(RemoteInvocationSubject::ClassRelation { relation_id })
        }
        "object_relation" | "objectrelation" => {
            let relation_id = input.relation_id.ok_or_else(|| {
                AppError::MissingOptions(vec!["relation-id".to_string()])
            })?;
            Ok(RemoteInvocationSubject::ObjectRelation { relation_id })
        }
        _ => Err(AppError::ParseError(format!(
            "Invalid subject kind '{}'. Valid values: namespace, class, object, class_relation, object_relation",
            input.subject_kind
        ))),
    }
}

impl HubuumGateway {
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
            namespace_id: input.namespace_id,
            name: input.name,
            description: input.description,
            method,
            url_template: input.url_template,
            allowed_subject_types,
            auth_config: input.auth_config.map(parse_auth_config),
            body_template: input.body_template,
            class_id: input.class_id,
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
            self.client.remote_targets().find().filters(filters),
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
        let target = self.client.remote_targets().select_by_name(name)?;
        Ok(RemoteTargetRecord::from(target.resource()))
    }

    pub fn update_remote_target(
        &self,
        input: UpdateRemoteTargetInput,
    ) -> Result<RemoteTargetRecord, AppError> {
        let target = self.client.remote_targets().select_by_name(&input.name)?;

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
            namespace_id: input.namespace_id,
            method,
            url_template: input.url_template,
            headers_template: input.headers_template,
            auth_config: input.auth_config.map(parse_auth_config),
            allowed_subject_types,
            body_template: input.body_template,
            class_id: input.class_id,
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
        let target = self.client.remote_targets().select_by_name(name)?;
        self.client.remote_targets().delete(target.id())?;
        Ok(())
    }

    pub fn invoke_remote_target(
        &self,
        name: &str,
        input: InvokeRemoteTargetInput,
    ) -> Result<TaskRecord, AppError> {
        let handle = self.client.remote_targets().select_by_name(name)?;
        let subject = build_invocation_subject(&input)?;
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
        "namespace_id",
        "namespace_id",
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
    SortFieldSpec::new("namespace_id", "namespace_id"),
    SortFieldSpec::new("enabled", "enabled"),
];
