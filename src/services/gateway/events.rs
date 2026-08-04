use std::fmt::{Display, Formatter};
use std::str::FromStr;

use hubuum_client::{
    client::sync::{EventListRequest, HistoryRequest},
    types::SortDirection,
    ClassId, CollectionId, EventDeliveryId, EventSinkId, EventSubscriptionId, ExportTemplateId,
    FilterOperator, GroupId, HistoryId, HubuumDateTime, NewEventSink, NewEventSubscription,
    ObjectId, QueryFilter, RemoteTargetId, UpdateEventSink, UpdateEventSubscription, UserId,
};
use log::debug;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{from_value, Value};

use crate::domain::{AuditEventId, AuditEventRecord, JsonRecord};
use crate::errors::AppError;
use crate::list_query::{
    fetch_cursor_results, fetch_query_results, validate_filter_clauses, validate_sort_clauses,
    FilterFieldSpec, FilterOperatorProfile, FilterValueProfile, ListQuery, PageSelection,
    PagedResult, SortFieldSpec, ValidatedFilterClause,
};

use super::HubuumGateway;

#[derive(Debug, Clone, Default)]
pub struct AuditListInput {
    pub action: Option<String>,
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

#[derive(Debug, Clone)]
pub enum HistoryScope {
    Class(ClassId),
    Object {
        class_id: ClassId,
        object_id: ObjectId,
    },
    ClassName(String),
    ObjectName {
        class_name: String,
        object_name: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct HistoryInput {
    pub limit: Option<usize>,
    pub sort: Option<String>,
    pub cursor: Option<String>,
    pub at: Option<String>,
    pub include_total: bool,
    pub page_selection: PageSelection,
}

impl HubuumGateway {
    pub fn list_event_sink_names(&self) -> Result<Vec<String>, AppError> {
        Ok(self
            .client
            .event_sinks()
            .query()
            .list()?
            .into_iter()
            .map(|sink| sink.name)
            .collect())
    }

    pub fn event_sink_id_by_name(&self, name: &str) -> Result<EventSinkId, AppError> {
        Ok(self.client.event_sinks().get_by_name(name)?.id())
    }

    pub fn list_event_subscription_names_for_collection(
        &self,
        collection: &str,
    ) -> Result<Vec<String>, AppError> {
        let collection_id = self.collection_id(collection)?;
        Ok(self
            .client
            .event_subscriptions(collection_id)
            .query()
            .limit(200)
            .page()?
            .items
            .into_iter()
            .map(|subscription| subscription.name)
            .collect())
    }

    pub fn collection_id_by_name(&self, name: &str) -> Result<CollectionId, AppError> {
        Ok(self.collection_id(name)?.into())
    }

    pub fn user_id_by_name(&self, name: &str) -> Result<UserId, AppError> {
        Ok(self.client.users().get_by_name(name)?.id())
    }

    pub fn audit_scope_by_name(
        &self,
        resource: AuditResourceKind,
        name: Option<&str>,
        class_name: Option<&str>,
    ) -> Result<AuditScope, AppError> {
        let name = name.ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;
        match resource {
            AuditResourceKind::Collection => {
                Ok(AuditScope::Collection(self.collection_id(name)?.into()))
            }
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
                self.client.users().get_by_name(name)?.id(),
            )),
            AuditResourceKind::Group => Ok(AuditScope::Group(
                self.client.groups().get_by_name(name)?.id(),
            )),
            AuditResourceKind::Template => Ok(AuditScope::Template(
                self.client.export_templates().get_by_name(name)?.id(),
            )),
            AuditResourceKind::RemoteTarget => Ok(AuditScope::RemoteTarget(
                self.client.remote_targets().get_by_name(name)?.id(),
            )),
        }
    }

    pub fn audit_events(
        &self,
        scope: AuditScope,
        input: AuditListInput,
    ) -> Result<PagedResult<AuditEventRecord>, AppError> {
        let request = match scope {
            AuditScope::Global => self.client.events(),
            AuditScope::Collection(id) => self.client.collection_events(id),
            AuditScope::Class(id) => self.client.class_events(id),
            AuditScope::Object {
                class_id,
                object_id,
            } => self.client.object_events(class_id, object_id),
            AuditScope::Template(id) => self.client.template_events(id),
            AuditScope::RemoteTarget(id) => self.client.remote_target_events(id),
            AuditScope::User(id) => self.client.user_events(id),
            AuditScope::Group(id) => self.client.group_events(id),
        };

        let request = apply_audit_input(request, &input)?;
        let page = if matches!(input.page_selection, PageSelection::All) {
            PagedResult::from_pages(request.pages())?
        } else {
            PagedResult::from_page(request.page()?, |event| event)
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
            self.client
                .users()
                .get(id)
                .map(|user| user.resource().name.clone())
                .map_err(|error| {
                    debug!("Unable to resolve audit actor user {id}: {error}");
                })
                .ok()
        });
        let collection = record.audit_collection_id().and_then(|id| {
            self.client
                .collections()
                .get(id)
                .map(|collection| collection.resource().name.clone())
                .map_err(|error| {
                    debug!("Unable to resolve audit collection {id}: {error}");
                })
                .ok()
        });

        record.with_audit_resource_names(actor_user, collection)
    }

    pub fn history(
        &self,
        scope: HistoryScope,
        input: HistoryInput,
    ) -> Result<PagedResult<JsonRecord>, AppError> {
        let scope = self.resolve_history_scope(scope)?;
        if let Some(at) = input.at.as_deref() {
            let record = self.history_record_at_resolved(scope, parse_hubuum_datetime(at)?)?;
            return Ok(PagedResult {
                items: vec![record],
                next_cursor: None,
                returned_count: 1,
                total_count: input.include_total.then_some(1),
            });
        }

        match scope {
            HistoryScope::Class(id) => {
                let request = apply_history_input(self.client.class_history(id), &input)?;
                history_results(request, &input)
            }
            HistoryScope::Object {
                class_id,
                object_id,
            } => {
                let request =
                    apply_history_input(self.client.object_history(class_id, object_id), &input)?;
                history_results(request, &input)
            }
            HistoryScope::ClassName(_) | HistoryScope::ObjectName { .. } => {
                unreachable!("history name scopes are resolved before request execution")
            }
        }
    }

    pub fn history_record_at(&self, scope: HistoryScope, at: &str) -> Result<JsonRecord, AppError> {
        let scope = self.resolve_history_scope(scope)?;
        self.history_record_at_resolved(scope, parse_hubuum_datetime(at)?)
    }

    pub fn history_record_by_id(
        &self,
        scope: HistoryScope,
        history_id: HistoryId,
    ) -> Result<JsonRecord, AppError> {
        const PAGE_LIMIT: usize = 100;
        const MAX_PAGES: usize = 100;

        let scope = self.resolve_history_scope(scope)?;
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = self.history(
                scope.clone(),
                HistoryInput {
                    limit: Some(PAGE_LIMIT),
                    sort: Some("-history_id".to_string()),
                    cursor: cursor.clone(),
                    ..HistoryInput::default()
                },
            )?;

            if let Some(record) = page
                .items
                .into_iter()
                .find(|record| json_record_history_id(record) == Some(history_id))
            {
                return Ok(record);
            }

            let Some(next_cursor) = page.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        Err(AppError::EntityNotFound(format!(
            "history record {history_id} not found in the first {} versions of the selected resource",
            PAGE_LIMIT * MAX_PAGES
        )))
    }

    fn history_record_at_resolved(
        &self,
        scope: HistoryScope,
        at: HubuumDateTime,
    ) -> Result<JsonRecord, AppError> {
        match scope {
            HistoryScope::Class(id) => {
                JsonRecord::from_serializable(self.client.class_history_as_of(id, at)?)
                    .map_err(AppError::from)
            }
            HistoryScope::Object {
                class_id,
                object_id,
            } => JsonRecord::from_serializable(
                self.client.object_history_as_of(class_id, object_id, at)?,
            )
            .map_err(AppError::from),
            HistoryScope::ClassName(_) | HistoryScope::ObjectName { .. } => {
                unreachable!("history name scopes are resolved before request execution")
            }
        }
    }

    fn resolve_history_scope(&self, scope: HistoryScope) -> Result<HistoryScope, AppError> {
        match scope {
            HistoryScope::ClassName(class_name) => Ok(HistoryScope::Class(
                self.class_handle_by_name(&class_name)?.id(),
            )),
            HistoryScope::ObjectName {
                class_name,
                object_name,
            } => {
                let object = self.object_handle_by_name(&class_name, &object_name)?;
                Ok(HistoryScope::Object {
                    class_id: object.resource().hubuum_class_id,
                    object_id: object.id(),
                })
            }
            other => Ok(other),
        }
    }

    pub fn event_sinks(&self, query: &ListQuery) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EVENT_SINK_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_SINK_SORT_SPECS)?;
        let filters = validated
            .iter()
            .map(|clause| self.resolve_validated_filter(clause))
            .collect::<Result<Vec<_>, _>>()?;
        let page = fetch_query_results(
            self.client.event_sinks().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        paged_to_json(page)
    }

    pub fn event_sink_by_name(&self, name: &str) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(
            self.client
                .event_sinks()
                .get_by_name(name)?
                .resource()
                .clone(),
        )
        .map_err(AppError::from)
    }

    pub fn create_event_sink(&self, input: NewEventSink) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(self.client.event_sinks().create_raw(input)?)
            .map_err(AppError::from)
    }

    pub fn update_event_sink(
        &self,
        name: &str,
        input: UpdateEventSink,
    ) -> Result<JsonRecord, AppError> {
        let sink = self.client.event_sinks().get_by_name(name)?;
        JsonRecord::from_serializable(self.client.event_sinks().update_raw(sink.id(), input)?)
            .map_err(AppError::from)
    }

    pub fn delete_event_sink_by_name(&self, name: &str) -> Result<(), AppError> {
        let sink = self.client.event_sinks().get_by_name(name)?;
        self.client.event_sinks().delete(sink.id())?;
        Ok(())
    }

    pub fn event_subscriptions(
        &self,
        collection_id: CollectionId,
        query: &ListQuery,
    ) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EVENT_SUBSCRIPTION_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_SUBSCRIPTION_SORT_SPECS)?;
        let filters = self.resolve_event_filters(&validated)?;
        let page = fetch_cursor_results(
            self.client
                .event_subscriptions(collection_id)
                .query()
                .filters(filters),
            query,
            &validated_sorts,
        )?;
        paged_to_json(page)
    }

    pub fn event_subscription(
        &self,
        collection_id: CollectionId,
        subscription_id: EventSubscriptionId,
    ) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(
            self.client
                .event_subscriptions(collection_id)
                .get(subscription_id)?,
        )
        .map_err(AppError::from)
    }

    pub fn event_subscription_by_name(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> Result<JsonRecord, AppError> {
        let subscription = self.event_subscription_id_by_name(collection_id, name)?;
        self.event_subscription(collection_id, subscription)
    }

    pub fn create_event_subscription(
        &self,
        collection_id: CollectionId,
        input: NewEventSubscription,
    ) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(
            self.client
                .event_subscriptions(collection_id)
                .create(input)?,
        )
        .map_err(AppError::from)
    }

    pub fn update_event_subscription(
        &self,
        collection_id: CollectionId,
        subscription_name: &str,
        input: UpdateEventSubscription,
    ) -> Result<JsonRecord, AppError> {
        let subscription_id =
            self.event_subscription_id_by_name(collection_id, subscription_name)?;
        JsonRecord::from_serializable(
            self.client
                .event_subscriptions(collection_id)
                .update(subscription_id, input)?,
        )
        .map_err(AppError::from)
    }

    pub fn delete_event_subscription(
        &self,
        collection_id: CollectionId,
        subscription_id: EventSubscriptionId,
    ) -> Result<(), AppError> {
        self.client
            .event_subscriptions(collection_id)
            .delete(subscription_id)?;
        Ok(())
    }

    pub fn delete_event_subscription_by_name(
        &self,
        collection_id: CollectionId,
        subscription_name: &str,
    ) -> Result<(), AppError> {
        let subscription_id =
            self.event_subscription_id_by_name(collection_id, subscription_name)?;
        self.delete_event_subscription(collection_id, subscription_id)
    }

    fn event_subscription_id_by_name(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> Result<EventSubscriptionId, AppError> {
        let page = self
            .client
            .event_subscriptions(collection_id)
            .query()
            .filter("name", FilterOperator::Equals { is_negated: false }, name)
            .limit(2)
            .page()?;
        match page.items.as_slice() {
            [subscription] => Ok(subscription.id),
            [] => Err(AppError::EntityNotFound(format!(
                "event subscription '{name}'"
            ))),
            _ => Err(AppError::MultipleEntitiesFound(format!(
                "event subscriptions named '{name}'"
            ))),
        }
    }

    pub fn event_deliveries(&self, query: &ListQuery) -> Result<PagedResult<JsonRecord>, AppError> {
        let validated = validate_filter_clauses(&query.filters, EVENT_DELIVERY_FILTER_SPECS)?;
        let validated_sorts = validate_sort_clauses(&query.sorts, EVENT_DELIVERY_SORT_SPECS)?;
        let filters = self.resolve_event_filters(&validated)?;
        let page = fetch_cursor_results(
            self.client.event_deliveries().query().filters(filters),
            query,
            &validated_sorts,
        )?;
        paged_to_json(page)
    }

    pub fn event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(self.client.event_deliveries().get(id)?)
            .map_err(AppError::from)
    }

    pub fn event_delivery_health(&self) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(self.client.event_deliveries().health()?)
            .map_err(AppError::from)
    }

    pub fn retry_event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(self.client.event_deliveries().retry(id)?)
            .map_err(AppError::from)
    }

    pub fn dead_event_delivery(&self, id: EventDeliveryId) -> Result<JsonRecord, AppError> {
        JsonRecord::from_serializable(self.client.event_deliveries().mark_dead(id)?)
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

fn json_record_history_id(record: &JsonRecord) -> Option<HistoryId> {
    from_value(record.value.get("history_id")?.clone()).ok()
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

fn apply_history_input<T>(
    mut request: HistoryRequest<T>,
    input: &HistoryInput,
) -> Result<HistoryRequest<T>, AppError>
where
    T: DeserializeOwned,
{
    if let Some(limit) = input.limit {
        request = request.limit(limit);
    }
    request = request.include_total(input.include_total);
    if let Some((field, direction)) = parse_single_sort(input.sort.as_deref())? {
        request = request.sort(field, direction);
    }
    if let Some(cursor) = &input.cursor {
        request = request.cursor(cursor);
    }
    Ok(request)
}

fn parse_single_sort(sort: Option<&str>) -> Result<Option<(&str, SortDirection)>, AppError> {
    let Some(sort) = sort.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if sort.contains(',') {
        return Err(AppError::InvalidOption(
            "only one sort field is supported by the official hubuum_client request type"
                .to_string(),
        ));
    }
    let sort = sort.trim();
    if let Some(field) = sort.strip_prefix('-') {
        Ok(Some((field, SortDirection::Desc)))
    } else {
        Ok(Some((sort, SortDirection::Asc)))
    }
}

fn parse_hubuum_datetime(value: &str) -> Result<HubuumDateTime, AppError> {
    from_value(Value::String(value.to_string())).map_err(AppError::from)
}

fn history_results<T>(
    request: HistoryRequest<T>,
    input: &HistoryInput,
) -> Result<PagedResult<JsonRecord>, AppError>
where
    T: DeserializeOwned + Serialize,
{
    let page = if matches!(input.page_selection, PageSelection::All) {
        PagedResult::from_pages(request.pages())?
    } else {
        PagedResult::from_page(request.page()?, |item| item)
    };
    paged_to_json(page)
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
