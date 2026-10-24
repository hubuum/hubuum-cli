use hubuum_client::{
    NewRemoteTarget, RemoteAuthConfig, RemoteHttpMethod, RemoteInvocationSubject,
    RemoteTargetInvokeRequest, RemoteTargetSubjectType, UpdateRemoteTarget,
};
use serde_json::Value;

use crate::diagnostics::client_call;
use crate::domain::{ObjectReference, RemoteTargetRecord, ResourceName, TaskRecord};
use crate::errors::AppError;
use crate::list_query::{
    fetch_query_results, validate_filter_clauses, validate_sort_clauses, FilterFieldSpec,
    FilterOperatorProfile, FilterValueProfile, ListQuery, PagedResult, SortFieldSpec,
};

use super::{HubuumGateway, RelationTarget};

#[derive(Debug, Clone)]
pub struct CreateRemoteTargetInput {
    pub collection: String,
    pub name: String,
    pub description: String,
    pub method: RemoteHttpMethod,
    pub url_template: String,
    pub allowed_subject_types: Vec<RemoteTargetSubjectType>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub headers_template: Option<Value>,
    pub timeout_ms: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct UpdateRemoteTargetInput {
    pub name: String,
    pub rename: Option<String>,
    pub description: Option<String>,
    pub collection: Option<String>,
    pub method: Option<RemoteHttpMethod>,
    pub url_template: Option<String>,
    pub allowed_subject_types: Option<Vec<RemoteTargetSubjectType>>,
    pub auth_config: Option<RemoteAuthConfigInput>,
    pub body_template: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub headers_template: Option<Value>,
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
pub enum InvocationSubject {
    Collection(ResourceName),
    Class(ResourceName),
    Object(ObjectReference),
    ClassRelation {
        from: ResourceName,
        to: ResourceName,
    },
    ObjectRelation {
        from: ObjectReference,
        to: ObjectReference,
    },
}

#[derive(Debug, Clone)]
pub struct InvokeRemoteTargetInput {
    subject: InvocationSubject,
    parameters: Option<Value>,
    body_override: Option<Value>,
}

impl InvokeRemoteTargetInput {
    pub fn new(subject: InvocationSubject) -> Self {
        Self {
            subject,
            parameters: None,
            body_override: None,
        }
    }
    pub fn parameters(mut self, value: Option<Value>) -> Self {
        self.parameters = value;
        self
    }
    pub fn body_override(mut self, value: Option<Value>) -> Self {
        self.body_override = value;
        self
    }
}

fn parse_auth_config(input: RemoteAuthConfigInput) -> RemoteAuthConfig {
    match input {
        RemoteAuthConfigInput::None => RemoteAuthConfig::None,
        RemoteAuthConfigInput::BearerSecret { secret } => RemoteAuthConfig::BearerSecret {
            secret: secret.into(),
        },
        RemoteAuthConfigInput::BasicSecret { username, secret } => RemoteAuthConfig::BasicSecret {
            username,
            secret: secret.into(),
        },
        RemoteAuthConfigInput::ApiKeySecret { header, secret } => RemoteAuthConfig::ApiKeySecret {
            header,
            secret: secret.into(),
        },
    }
}

impl InvocationSubject {
    fn resolve(&self, gateway: &HubuumGateway) -> Result<RemoteInvocationSubject, AppError> {
        Ok(match self {
            Self::Collection(name) => RemoteInvocationSubject::Collection {
                collection_id: gateway.collection_id(name.as_str())?,
            },
            Self::Class(name) => RemoteInvocationSubject::Class {
                class_id: gateway.class_handle_by_name(name.as_str())?.id(),
            },
            Self::Object(reference) => {
                let object =
                    gateway.object_handle_by_name(reference.class(), reference.object())?;
                RemoteInvocationSubject::Object {
                    class_id: object.resource().hubuum_class_id,
                    object_id: object.id(),
                }
            }
            Self::ClassRelation { from, to } => RemoteInvocationSubject::ClassRelation {
                relation_id: gateway
                    .get_class_relation_by_pair(from.as_str(), to.as_str())?
                    .id
                    .try_into()?,
            },
            Self::ObjectRelation { from, to } => RemoteInvocationSubject::ObjectRelation {
                relation_id: gateway
                    .get_object_relation_v2(&RelationTarget {
                        class_a: from.class().to_owned(),
                        class_b: to.class().to_owned(),
                        object_a: Some(from.object().to_owned()),
                        object_b: Some(to.object().to_owned()),
                    })?
                    .id
                    .try_into()?,
            },
        })
    }
}

impl HubuumGateway {
    pub fn list_remote_target_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_remote_targets(&ListQuery {
                limit: Some(200.try_into().expect("positive page size")),
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
        let new_target = NewRemoteTarget {
            collection_id: self.collection_id(&input.collection)?,
            name: input.name,
            description: input.description,
            method: input.method,
            url_template: input.url_template,
            allowed_subject_types: input.allowed_subject_types,
            auth_config: input.auth_config.map(parse_auth_config),
            body_template: input.body_template,
            class_id: input
                .class
                .as_deref()
                .map(|class| self.class_handle_by_name(class).map(|handle| handle.id()))
                .transpose()?,
            enabled: input.enabled,
            headers_template: input.headers_template,
            timeout_ms: input.timeout_ms,
        };

        let target = client_call("remote_targets.create_raw", || {
            self.client().remote_targets().create_raw(new_target)
        })?;
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

        let page = fetch_query_results(
            self.client().remote_targets().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        Ok(page.map(RemoteTargetRecord::from))
    }

    pub fn remote_target(&self, name: &str) -> Result<RemoteTargetRecord, AppError> {
        let target = client_call("remote_targets.get_by_name", || {
            self.client().remote_targets().get_by_name(name)
        })?;
        Ok(RemoteTargetRecord::from(target.resource()))
    }

    pub fn update_remote_target(
        &self,
        input: UpdateRemoteTargetInput,
    ) -> Result<RemoteTargetRecord, AppError> {
        let target = client_call("remote_targets.get_by_name", || {
            self.client().remote_targets().get_by_name(&input.name)
        })?;

        let update = UpdateRemoteTarget {
            name: input.rename,
            description: input.description,
            collection_id: input
                .collection
                .as_deref()
                .map(|collection| self.collection_id(collection))
                .transpose()?,
            method: input.method,
            url_template: input.url_template,
            headers_template: input.headers_template,
            auth_config: input.auth_config.map(parse_auth_config),
            allowed_subject_types: input.allowed_subject_types,
            body_template: input.body_template,
            class_id: input
                .class
                .as_deref()
                .map(|class| self.class_handle_by_name(class).map(|handle| handle.id()))
                .transpose()?,
            enabled: input.enabled,
            timeout_ms: input.timeout_ms,
        };

        let updated = client_call("remote_targets.update.send", || {
            self.client()
                .remote_targets()
                .update(target.id())
                .params(update)
                .send()
        })?;
        Ok(RemoteTargetRecord::from(updated))
    }

    pub fn delete_remote_target(&self, name: &str) -> Result<(), AppError> {
        let target = client_call("remote_targets.get_by_name", || {
            self.client().remote_targets().get_by_name(name)
        })?;
        client_call("remote_targets.delete", || {
            self.client().remote_targets().delete(target.id())
        })?;
        Ok(())
    }

    pub fn invoke_remote_target(
        &self,
        name: &str,
        input: InvokeRemoteTargetInput,
    ) -> Result<TaskRecord, AppError> {
        let handle = client_call("remote_targets.get_by_name", || {
            self.client().remote_targets().get_by_name(name)
        })?;
        let subject = input.subject.resolve(self)?;
        let mut req = RemoteTargetInvokeRequest::new(subject);
        if let Some(p) = input.parameters {
            req = req.parameters(p);
        }
        if let Some(b) = input.body_override {
            req = req.body_override(b);
        }
        Ok(TaskRecord(client_call("remote_targets.invoke", || {
            handle.invoke(req)
        })?))
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
