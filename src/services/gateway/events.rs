use std::fmt::{Display, Formatter};
use std::str::FromStr;

use hubuum_client::{
    client::sync::EventListRequest, ClassId, CollectionId, EventDeliveryId,
    EventNotificationRequest, EventSinkId, EventSubscriptionId, ExportTemplateId, FilterOperator,
    GroupId, NewEventSink, NewEventSubscription, ObjectId, QueryFilter, RemoteTargetId,
    UpdateEventSink, UpdateEventSubscription, UserId,
};
use log::debug;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::diagnostics::client_call;
use crate::domain::{AuditEventId, AuditEventRecord, JsonRecord};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, fetch_query_results, validate_filter_clauses, validate_sort_clauses,
    FilterFieldSpec, FilterOperatorProfile, FilterValueProfile, ListQuery, PageSelection,
    PagedResult, SortFieldSpec, ValidatedFilterClause,
};

use super::{shared::parse_single_sort, HubuumGateway};

#[derive(Debug, Clone, Default)]
pub struct AuditListInput {
    pub action: Option<String>,
    pub include_relations: bool,
    pub actor_kind: Option<AuditActorKind>,
    pub actor_user_id: Option<UserId>,
    pub collection_id: Option<CollectionId>,
    pub occurred_after: Option<String>,
    pub occurred_before: Option<String>,
    pub limit: Option<usize>,
    pub sort: Option<String>,
    pub cursor: Option<String>,
    pub page_selection: PageSelection,
}

#[derive(Debug, Clone, Copy)]
pub enum EventSubscriptionScope {
    Collection(CollectionId),
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActorKind {
    User,
    System,
    Worker,
}

impl AuditActorKind {
    pub const VALUES: &[&str] = &["user", "system", "worker"];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
            Self::Worker => "worker",
        }
    }
}

impl Display for AuditActorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AuditActorKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "system" => Ok(Self::System),
            "worker" => Ok(Self::Worker),
            _ => Err(AppError::InvalidOption(format!(
                "Invalid audit actor kind '{value}'. Valid values: {}",
                Self::VALUES.join(", ")
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditResourceKind {
    Collection,
    Class,
    Object,
    User,
    Group,
    Template,
    RemoteTarget,
}

impl AuditResourceKind {
    pub const VALUES: &[&str] = &[
        "collection",
        "class",
        "object",
        "user",
        "group",
        "template",
        "remote-target",
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Class => "class",
            Self::Object => "object",
            Self::User => "user",
            Self::Group => "group",
            Self::Template => "template",
            Self::RemoteTarget => "remote-target",
        }
    }
}

impl Display for AuditResourceKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AuditResourceKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "collection" => Ok(Self::Collection),
            "class" => Ok(Self::Class),
            "object" => Ok(Self::Object),
            "user" => Ok(Self::User),
            "group" => Ok(Self::Group),
            "template" => Ok(Self::Template),
            "remote-target" => Ok(Self::RemoteTarget),
            _ => Err(AppError::InvalidOption(format!(
                "Invalid audit resource kind '{value}'. Valid values: {}",
                Self::VALUES.join(", ")
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AuditScope {
    Global,
    Collection(CollectionId),
    Class(ClassId),
    Object {
        class_id: ClassId,
        object_id: ObjectId,
    },
    User(UserId),
    Group(GroupId),
    Template(ExportTemplateId),
    RemoteTarget(RemoteTargetId),
}

impl HubuumGateway {
    pub fn list_event_sink_names(&self) -> Result<Vec<String>, AppError> {
        Ok(client_call("event_sinks.list", || {
            self.client().event_sinks().query().list()
        })?
        .into_iter()
        .map(|sink| sink.name)
        .collect())
    }

    pub fn event_sink_id_by_name(&self, name: &str) -> Result<EventSinkId, AppError> {
        Ok(client_call("event_sinks.get_by_name", || {
            self.client().event_sinks().get_by_name(name)
        })?
        .id())
    }

    pub fn list_event_subscription_names_for_collection(
        &self,
        collection: &str,
    ) -> Result<Vec<String>, AppError> {
        let collection_id = self.collection_id(collection)?;
        Ok(client_call("event_subscriptions.page", || {
            self.client()
                .event_subscriptions(collection_id)
                .query()
                .limit(200)
                .page()
        })?
        .items
        .into_iter()
        .map(|subscription| subscription.name)
        .collect())
    }

    pub fn collection_id_by_name(&self, name: &str) -> Result<CollectionId, AppError> {
        self.collection_id(name)
    }

    pub fn user_id_by_name(&self, name: &str) -> Result<UserId, AppError> {
        Ok(client_call("users.get_by_name", || {
            self.client().users().get_by_name(name)
        })?
        .id())
    }

    pub fn audit_scope_by_name(
        &self,
        resource: AuditResourceKind,
        name: Option<&str>,
        class_name: Option<&str>,
    ) -> Result<AuditScope, AppError> {
        let name = name.ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        match resource {
            AuditResourceKind::Collection => Ok(AuditScope::Collection(self.collection_id(name)?)),
            AuditResourceKind::Class => {
                Ok(AuditScope::Class(self.class_handle_by_name(name)?.id()))
            }
            AuditResourceKind::Object => {
                let class_name = class_name
                    .ok_or_else(|| AppError::MissingOptions(vec!["class".to_string()]))?;
                let object = self.object_handle_by_name(class_name, name)?;
                Ok(AuditScope::Object {
                    class_id: object.resource().hubuum_class_id,
                    object_id: object.id(),
                })
            }
            AuditResourceKind::User => Ok(AuditScope::User(
                client_call("users.get_by_name", || {
                    self.client().users().get_by_name(name)
                })?
                .id(),
            )),
            AuditResourceKind::Group => Ok(AuditScope::Group(
                client_call("groups.get_by_name", || {
                    self.client().groups().get_by_name(name)
                })?
                .id(),
            )),
            AuditResourceKind::Template => Ok(AuditScope::Template(
                client_call("export_templates.get_by_name", || {
                    self.client().export_templates().get_by_name(name)
                })?
                .id(),
            )),
            AuditResourceKind::RemoteTarget => Ok(AuditScope::RemoteTarget(
                client_call("remote_targets.get_by_name", || {
                    self.client().remote_targets().get_by_name(name)
                })?
                .id(),
            )),
        }
    }

    pub fn audit_events(
        &self,
        scope: AuditScope,
        input: AuditListInput,
    ) -> Result<PagedResult<AuditEventRecord>, AppError> {
        if input.include_relations
            && !matches!(scope, AuditScope::Class(_) | AuditScope::Object { .. })
        {
            return Err(AppError::InvalidOption(
                "--include-relations requires a class or object audit resource".into(),
            ));
        }
        let request = match scope {
            AuditScope::Global => self.client().events(),
            AuditScope::Collection(id) => self.client().collection_events(id),
            AuditScope::Class(id) => self.client().class_events(id),
            AuditScope::Object {
                class_id,
                object_id,
            } => self.client().object_events(class_id, object_id),
            AuditScope::Template(id) => self.client().template_events(id),
            AuditScope::RemoteTarget(id) => self.client().remote_target_events(id),
            AuditScope::User(id) => self.client().user_events(id),
            AuditScope::Group(id) => self.client().group_events(id),
        };

        let request = apply_audit_input(request, &input)?;
        let page = if matches!(input.page_selection, PageSelection::All) {
            PagedResult::from_pages(request.pages())?
        } else {
            PagedResult::from_page(
                client_call("audit_events.page", || request.page())?,
                |event| event,
            )
        };
        Ok(page.map(AuditEventRecord::from))
    }

    pub fn audit_event_by_id(&self, id: AuditEventId) -> Result<JsonRecord, AppError> {
        const PAGE_LIMIT: usize = 100;
        const MAX_PAGES: usize = 100;

        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = self.audit_events(
                AuditScope::Global,
                AuditListInput {
                    limit: Some(PAGE_LIMIT),
                    sort: Some("-occurred_at".to_string()),
                    cursor: cursor.clone(),
                    ..AuditListInput::default()
                },
            )?;

            if let Some(record) = page
                .items
                .into_iter()
                .find(|record| record.id() == id.get())
            {
                return Ok(
                    self.resolve_audit_resource_names(JsonRecord::from_serializable(record.0)?)
                );
            }

            let Some(next_cursor) = page.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        Err(AppError::EntityNotFound(format!(
            "audit event {id} not found in the first {} visible events",
            PAGE_LIMIT * MAX_PAGES
        )))
    }

    fn resolve_audit_resource_names(&self, record: JsonRecord) -> JsonRecord {
        let actor_user = record.audit_actor_user_id().and_then(|id| {
            client_call("users.get", || self.client().users().get(id))
                .map(|user| user.resource().name.clone())
                .map_err(|error| {
                    debug!("Unable to resolve audit actor user {id}: {error}");
                })
                .ok()
        });
        let collection = record.audit_collection_id().and_then(|id| {
            client_call("collections.get", || self.client().collections().get(id))
                .map(|collection| collection.resource().name.clone())
                .map_err(|error| {
                    debug!("Unable to resolve audit collection {id}: {error}");
                })
                .ok()
        });

        record.with_audit_resource_names(actor_user, collection)
    }

    pub fn collection_event_sinks(
        &self,
        collection: CollectionId,
        query: &ListQuery,
    ) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated =
            validate_filter_clauses(&query.filters, COLLECTION_EVENT_SINK_FILTER_SPECS)?;
        let sorts = validate_sort_clauses(&query.sorts, COLLECTION_EVENT_SINK_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        paged_to_json(fetch_query_results(
            self.client()
                .collection_event_sinks(collection)
                .query()
                .filters(filters),
            query,
            &sorts,
        )?)
    }

    pub fn collection_event_sink_id_by_name(
        &self,
        collection: CollectionId,
        name: &str,
    ) -> Result<EventSinkId, AppError> {
        Ok(client_call("collection_event_sinks.get_by_name", || {
            self.client()
                .collection_event_sinks(collection)
                .get_by_name(name)
        })?
        .id())
    }

    pub fn subscription_sink_id_by_name(
        &self,
        collection: CollectionId,
        name: &str,
    ) -> Result<EventSinkId, AppError> {
        match client_call("collection_event_sinks.get_by_name", || {
            self.client()
                .collection_event_sinks(collection)
                .get_by_name(name)
        }) {
            Ok(sink) => Ok(sink.id()),
            // Preserve administrator workflows on v0.0.17, which has no scoped
            // discovery route. Never fall back after a permission denial.
            Err(error) if error.is_status(StatusCode::NOT_FOUND) => {
                self.event_sink_id_by_name(name)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn collection_event_sink_by_name(
        &self,
        collection: CollectionId,
        name: &str,
    ) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(
            client_call("collection_event_sinks.get_by_name", || {
                self.client()
                    .collection_event_sinks(collection)
                    .get_by_name(name)
            })?
            .resource()
            .clone(),
        )
        .map_err(AppError::from)
    }

    pub fn create_collection_event_sink(
        &self,
        collection: CollectionId,
        input: NewEventSink,
    ) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("collection_event_sinks.create_raw", || {
            self.client()
                .collection_event_sinks(collection)
                .create_raw(input)
        })?)
        .map_err(AppError::from)
    }

    pub fn update_collection_event_sink(
        &self,
        collection: CollectionId,
        name: &str,
        input: UpdateEventSink,
    ) -> Result<JsonRecord, AppError> {
        let id = self.collection_event_sink_id_by_name(collection, name)?;
        JsonRecord::from_serializable(client_call("collection_event_sinks.update_raw", || {
            self.client()
                .collection_event_sinks(collection)
                .update_raw(id, input)
        })?)
        .map_err(AppError::from)
    }

    pub fn delete_collection_event_sink(
        &self,
        collection: CollectionId,
        name: &str,
    ) -> Result<(), AppError> {
        let id = self.collection_event_sink_id_by_name(collection, name)?;
        client_call("collection_event_sinks.delete", || {
            self.client().collection_event_sinks(collection).delete(id)
        })?;
        Ok(())
    }

    pub fn grant_event_sink(&self, name: &str, collection: CollectionId) -> Result<(), AppError> {
        let sink = self.event_sink_id_by_name(name)?;
        client_call("client.grant_event_sink", || {
            self.client().grant_event_sink(sink, collection)
        })?;
        Ok(())
    }

    pub fn revoke_event_sink(&self, name: &str, collection: CollectionId) -> Result<(), AppError> {
        let sink = self.event_sink_id_by_name(name)?;
        client_call("client.revoke_event_sink", || {
            self.client().revoke_event_sink(sink, collection)
        })?;
        Ok(())
    }

    pub fn event_sink_collections(&self, name: &str) -> Result<JsonRecord, AppError> {
        let sink = self.event_sink_id_by_name(name)?;
        let collections = client_call("client.event_sink_collections", || {
            self.client().event_sink_collections(sink)
        })?;
        JsonRecord::from_serializable(serde_json::json!({"collections": collections}))
            .map_err(AppError::from)
    }

    pub fn event_sinks(&self, query: &ListQuery) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EVENT_SINK_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_SINK_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = fetch_query_results(
            self.client().event_sinks().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        paged_to_json(page)
    }

    pub fn event_sink_by_name(&self, name: &str) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(
            client_call("event_sinks.get_by_name", || {
                self.client().event_sinks().get_by_name(name)
            })?
            .resource()
            .clone(),
        )
        .map_err(AppError::from)
    }

    pub fn create_event_sink(&self, input: NewEventSink) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("event_sinks.create_raw", || {
            self.client().event_sinks().create_raw(input)
        })?)
        .map_err(AppError::from)
    }

    pub fn update_event_sink(
        &self,
        name: &str,
        input: UpdateEventSink,
    ) -> Result<JsonRecord, AppError> {
        let sink = client_call("event_sinks.get_by_name", || {
            self.client().event_sinks().get_by_name(name)
        })?;
        JsonRecord::from_serializable(client_call("event_sinks.update_raw", || {
            self.client().event_sinks().update_raw(sink.id(), input)
        })?)
        .map_err(AppError::from)
    }

    pub fn delete_event_sink_by_name(&self, name: &str) -> Result<(), AppError> {
        let sink = client_call("event_sinks.get_by_name", || {
            self.client().event_sinks().get_by_name(name)
        })?;
        client_call("event_sinks.delete", || {
            self.client().event_sinks().delete(sink.id())
        })?;
        Ok(())
    }

    pub fn event_subscriptions(
        &self,
        scope: EventSubscriptionScope,
        query: &ListQuery,
    ) -> Result<PagedResult<JsonRecord>, AppError> {
        let EventSubscriptionScope::Collection(collection_id) = scope else {
            if !query.filters.is_empty()
                || !query.sorts.is_empty()
                || query.limit.is_some()
                || query.cursor.is_some()
            {
                return Err(AppError::InvalidOption("System subscriptions return the complete list; --where, --sort, --limit and --cursor are unsupported. Use a local output pipeline to filter or sort.".into()));
            }
            let items = client_call("system_event_subscriptions.list", || {
                self.client().system_event_subscriptions().list()
            })?
            .into_iter()
            .map(JsonRecord::from_serializable)
            .collect::<Result<Vec<_>, _>>()?;
            return Ok(PagedResult {
                returned_count: items.len(),
                total_count: query.include_total.then_some(items.len() as u64),
                items,
                next_cursor: None,
            });
        };
        let validated = validate_filter_clauses(&query.filters, EVENT_SUBSCRIPTION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_SUBSCRIPTION_SORT_SPECS)?;
        let filters = self.resolve_event_filters(&validated)?;
        paged_to_json(fetch_cursor_results(
            self.client()
                .event_subscriptions(collection_id)
                .query()
                .filters(filters),
            query,
            &validated_sorts,
        )?)
    }

    pub fn event_subscription_by_name(
        &self,
        scope: EventSubscriptionScope,
        name: &str,
    ) -> Result<JsonRecord, AppError> {
        let id = self.event_subscription_id_by_name(scope, name)?;
        match scope {
            EventSubscriptionScope::Collection(collection) => Ok(JsonRecord::from_serializable(
                client_call("event_subscriptions.get", || {
                    self.client().event_subscriptions(collection).get(id)
                })?,
            )?),
            EventSubscriptionScope::System => Ok(JsonRecord::from_serializable(client_call(
                "system_event_subscriptions.get",
                || self.client().system_event_subscriptions().get(id),
            )?)?),
        }
    }

    pub fn create_event_subscription(
        &self,
        scope: EventSubscriptionScope,
        input: NewEventSubscription,
    ) -> Result<JsonRecord, AppError> {
        match scope {
            EventSubscriptionScope::Collection(collection) => Ok(JsonRecord::from_serializable(
                client_call("event_subscriptions.create", || {
                    self.client().event_subscriptions(collection).create(input)
                })?,
            )?),
            EventSubscriptionScope::System => Ok(JsonRecord::from_serializable(client_call(
                "system_event_subscriptions.create",
                || self.client().system_event_subscriptions().create(input),
            )?)?),
        }
    }

    pub fn update_event_subscription(
        &self,
        scope: EventSubscriptionScope,
        name: &str,
        input: UpdateEventSubscription,
    ) -> Result<JsonRecord, AppError> {
        let id = self.event_subscription_id_by_name(scope, name)?;
        match scope {
            EventSubscriptionScope::Collection(collection) => Ok(JsonRecord::from_serializable(
                client_call("event_subscriptions.update", || {
                    self.client()
                        .event_subscriptions(collection)
                        .update(id, input)
                })?,
            )?),
            EventSubscriptionScope::System => Ok(JsonRecord::from_serializable(client_call(
                "system_event_subscriptions.update",
                || self.client().system_event_subscriptions().update(id, input),
            )?)?),
        }
    }

    pub fn delete_event_subscription_by_name(
        &self,
        scope: EventSubscriptionScope,
        name: &str,
    ) -> Result<(), AppError> {
        let id = self.event_subscription_id_by_name(scope, name)?;
        match scope {
            EventSubscriptionScope::Collection(collection) => {
                client_call("event_subscriptions.delete", || {
                    self.client().event_subscriptions(collection).delete(id)
                })?
            }
            EventSubscriptionScope::System => {
                client_call("system_event_subscriptions.delete", || {
                    self.client().system_event_subscriptions().delete(id)
                })?
            }
        }
        Ok(())
    }

    fn event_subscription_id_by_name(
        &self,
        scope: EventSubscriptionScope,
        name: &str,
    ) -> Result<EventSubscriptionId, AppError> {
        let ids: Vec<_> = match scope {
            EventSubscriptionScope::Collection(collection) => {
                client_call("event_subscriptions.page", || {
                    self.client()
                        .event_subscriptions(collection)
                        .query()
                        .filter("name", FilterOperator::Equals { is_negated: false }, name)
                        .limit(2)
                        .page()
                })?
                .items
                .into_iter()
                .map(|subscription| subscription.id)
                .collect()
            }
            EventSubscriptionScope::System => {
                client_call("system_event_subscriptions.list", || {
                    self.client().system_event_subscriptions().list()
                })?
                .into_iter()
                .filter(|subscription| subscription.name == name)
                .map(|subscription| subscription.id)
                .collect()
            }
        };
        match ids.as_slice() {
            [id] => Ok(*id),
            [] => Err(AppError::EntityNotFound(format!(
                "event subscription '{name}'"
            ))),
            _ => Err(AppError::MultipleEntitiesFound(format!(
                "event subscriptions named '{name}'"
            ))),
        }
    }

    pub fn list_system_event_subscription_names(&self) -> Result<Vec<String>, AppError> {
        Ok(client_call("system_event_subscriptions.list", || {
            self.client().system_event_subscriptions().list()
        })?
        .into_iter()
        .map(|subscription| subscription.name)
        .collect())
    }

    pub fn preview_event_notification(
        &self,
        sink: EventSinkId,
        request: EventNotificationRequest,
    ) -> Result<JsonRecord, AppError> {
        Ok(JsonRecord::from_serializable(client_call(
            "client.preview_event_notification",
            || self.client().preview_event_notification(sink, request),
        )?)?)
    }

    pub fn test_event_notification(
        &self,
        sink: EventSinkId,
        request: EventNotificationRequest,
    ) -> Result<JsonRecord, AppError> {
        Ok(JsonRecord::from_serializable(client_call(
            "client.test_event_notification",
            || self.client().test_event_notification(sink, request),
        )?)?)
    }

    pub fn event_deliveries(&self, query: &ListQuery) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EVENT_DELIVERY_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_DELIVERY_SORT_SPECS)?;
        let filters = self.resolve_event_filters(&validated)?;
        let page = fetch_cursor_results(
            self.client().event_deliveries().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        paged_to_json(page)
    }

    pub fn event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("event_deliveries.get", || {
            self.client().event_deliveries().get(id)
        })?)
        .map_err(AppError::from)
    }

    pub fn event_delivery_health(&self) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("event_deliveries.health", || {
            self.client().event_deliveries().health()
        })?)
        .map_err(AppError::from)
    }

    pub fn retry_event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("event_deliveries.retry", || {
            self.client().event_deliveries().retry(id)
        })?)
        .map_err(AppError::from)
    }

    pub fn dead_event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(client_call("event_deliveries.mark_dead", || {
            self.client().event_deliveries().mark_dead(id)
        })?)
        .map_err(AppError::from)
    }

    fn resolve_event_filters(
        &self,
        validated: &[ValidatedFilterClause],
    ) -> Result<Vec<QueryFilter>, AppError> {
        validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect()
    }
}

fn apply_audit_input(
    mut request: EventListRequest,
    input: &AuditListInput,
) -> Result<EventListRequest, AppError> {
    if let Some(action) = &input.action {
        request = request.action(action);
    }
    if let Some(actor_kind) = input.actor_kind {
        request = request.actor_kind(actor_kind.as_str());
    }
    if input.include_relations {
        request = request.include_relations(true);
    }
    if let Some(actor_user_id) = input.actor_user_id {
        request = request.actor_user_id(actor_user_id);
    }
    if let Some(collection_id) = input.collection_id {
        request = request.collection_id(collection_id);
    }
    if let Some(occurred_after) = &input.occurred_after {
        request = request.occurred_after(occurred_after);
    }
    if let Some(occurred_before) = &input.occurred_before {
        request = request.occurred_before(occurred_before);
    }
    if let Some(limit) = input.limit {
        request = request.limit(limit);
    }
    if let Some((field, direction)) = parse_single_sort(input.sort.as_deref())? {
        request = request.sort(field, direction);
    }
    if let Some(cursor) = &input.cursor {
        request = request.cursor(cursor);
    }
    Ok(request)
}

fn paged_to_json<T: Serialize>(page: PagedResult<T>) -> Result<PagedResult<JsonRecord>, AppError> {
    let items = page
        .items
        .into_iter()
        .map(JsonRecord::from_serializable)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PagedResult {
        items,
        next_cursor: page.next_cursor,
        returned_count: page.returned_count,
        total_count: page.total_count,
    })
}

const COLLECTION_EVENT_SINK_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "kind",
        "kind",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "revision",
        "revision",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
];
const COLLECTION_EVENT_SINK_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("kind", "kind"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("revision", "revision"),
];

pub(crate) const EVENT_SINK_FILTER_SPECS: &[FilterFieldSpec] = &[
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
        "kind",
        "kind",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "enabled",
        "enabled",
        FilterOperatorProfile::Boolean,
        FilterValueProfile::Boolean,
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

pub(crate) const EVENT_SINK_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("kind", "kind"),
    SortFieldSpec::new("enabled", "enabled"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

pub(crate) const EVENT_SUBSCRIPTION_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "sink_id",
        "sink_id",
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
        "enabled",
        "enabled",
        FilterOperatorProfile::Boolean,
        FilterValueProfile::Boolean,
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

pub(crate) const EVENT_SUBSCRIPTION_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("sink_id", "sink_id"),
    SortFieldSpec::new("name", "name"),
    SortFieldSpec::new("enabled", "enabled"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

pub(crate) const EVENT_DELIVERY_FILTER_SPECS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "event_id",
        "event_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "subscription_id",
        "subscription_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "status",
        "status",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "attempts",
        "attempts",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "next_attempt_at",
        "next_attempt_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
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

pub(crate) const EVENT_DELIVERY_SORT_SPECS: &[SortFieldSpec] = &[
    SortFieldSpec::new("id", "id"),
    SortFieldSpec::new("event_id", "event_id"),
    SortFieldSpec::new("subscription_id", "subscription_id"),
    SortFieldSpec::new("status", "status"),
    SortFieldSpec::new("attempts", "attempts"),
    SortFieldSpec::new("next_attempt_at", "next_attempt_at"),
    SortFieldSpec::new("created_at", "created_at"),
    SortFieldSpec::new("updated_at", "updated_at"),
];

#[cfg(test)]
mod tests {
    use super::{AuditActorKind, AuditResourceKind};

    #[test]
    fn audit_actor_kind_validates_the_server_vocabulary() {
        assert_eq!(
            "worker"
                .parse::<AuditActorKind>()
                .expect("valid actor kind"),
            AuditActorKind::Worker
        );
        assert_eq!(
            "SYSTEM"
                .parse::<AuditActorKind>()
                .expect("case insensitive"),
            AuditActorKind::System
        );
        assert!("task".parse::<AuditActorKind>().is_err());
    }

    #[test]
    fn audit_resource_kind_validates_endpoint_scopes() {
        assert_eq!(
            "remote-target"
                .parse::<AuditResourceKind>()
                .expect("valid resource kind"),
            AuditResourceKind::RemoteTarget
        );
        assert!("task".parse::<AuditResourceKind>().is_err());
    }
}
