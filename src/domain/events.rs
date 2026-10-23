use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{from_value, json, Map, Value};

use hubuum_client::{CollectionId, EventResponse, ProvenancePrincipal, UserId};

use super::JsonRecord;
use crate::errors::AppError;
use crate::formatting::TableRenderable;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct AuditEventId(i64);

impl AuditEventId {
    pub fn new(value: i64) -> Result<Self, AppError> {
        if value <= 0 {
            return Err(AppError::InvalidOption(format!(
                "audit event ID must be positive, got {value}"
            )));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Display for AuditEventId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for AuditEventId {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = value.parse::<i64>().map_err(|_| {
            AppError::InvalidOption(format!(
                "Invalid audit event ID '{value}': expected a positive integer"
            ))
        })?;
        Self::new(parsed)
    }
}

impl TryFrom<i64> for AuditEventId {
    type Error = AppError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AuditEventId> for i64 {
    fn from(value: AuditEventId) -> Self {
        value.get()
    }
}

/// Public event UUID, distinct from the numeric audit row identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EventUuid(String);

impl FromStr for EventUuid {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        {
            return Err(AppError::InvalidOption("--event requires the event_id UUID from audit output, for example 11111111-1111-4111-8111-111111111111".into()));
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for EventUuid {
    type Error = AppError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<EventUuid> for String {
    fn from(value: EventUuid) -> Self {
        value.0
    }
}

impl Display for EventUuid {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

transparent_record!(AuditEventRecord, EventResponse);

impl AuditEventRecord {
    pub const fn id(&self) -> i64 {
        self.0.id
    }

    fn actor_label(&self) -> String {
        let actor_name = self
            .0
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.actor.principal.as_ref())
            .and_then(|principal| principal.name.as_deref());

        match actor_name {
            Some(name) => format!("{}: {name}", self.0.actor_kind),
            None => self.0.actor_kind.clone(),
        }
    }
}

impl TableRenderable for AuditEventRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "type", "action", "actor", "summary", "occurred"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.0.id.to_string(),
            self.0.entity_type.clone(),
            self.0.action.clone(),
            self.actor_label(),
            self.0.summary.clone(),
            self.0.occurred_at.to_string(),
        ]
    }
}

impl JsonRecord {
    pub(crate) fn into_audit_detail(mut self, include_snapshots: bool) -> Self {
        let initiator = self.audit_provenance_initiator();
        let generated_diff = match &self.value {
            Value::Object(object) if !object.contains_key("diff") => {
                match (object.get("before"), object.get("after")) {
                    (Some(before), Some(after)) => {
                        Some(nested_json_diff(before, after).unwrap_or_else(empty_json_object))
                    }
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(object) = self.value.as_object_mut() {
            if let Some(initiator) = initiator {
                object
                    .entry("initiator_principal_id")
                    .or_insert_with(|| Value::from(initiator.principal_id.get()));
                if let Some(name) = initiator.name {
                    object
                        .entry("initiator")
                        .or_insert_with(|| Value::String(name));
                }
            }
            if let Some(generated_diff) = generated_diff {
                object.insert("diff".to_string(), generated_diff);
            }
            if !include_snapshots {
                object.remove("before");
                object.remove("after");
            }
        }

        self
    }

    pub(crate) fn audit_actor_user_id(&self) -> Option<UserId> {
        from_value(self.value.get("actor_user_id")?.clone()).ok()
    }

    pub(crate) fn audit_collection_id(&self) -> Option<CollectionId> {
        from_value(self.value.get("collection_id")?.clone()).ok()
    }

    pub(crate) fn with_audit_resource_names(
        mut self,
        actor_user: Option<String>,
        collection: Option<String>,
    ) -> Self {
        if let Some(object) = self.value.as_object_mut() {
            if let Some(actor_user) = actor_user {
                object
                    .entry("actor_user")
                    .or_insert_with(|| Value::String(actor_user));
            }
            if let Some(collection) = collection {
                object
                    .entry("collection")
                    .or_insert_with(|| Value::String(collection));
            }
        }
        self
    }

    fn audit_provenance_initiator(&self) -> Option<ProvenancePrincipal> {
        from_value(self.value.pointer("/provenance/initiator")?.clone()).ok()
    }
}

fn nested_json_diff(before: &Value, after: &Value) -> Option<Value> {
    if before == after {
        return None;
    }

    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            let mut keys = before.keys().chain(after.keys()).collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();

            let mut changes = Map::new();
            for key in keys {
                let change = match (before.get(key), after.get(key)) {
                    (Some(before), Some(after)) => nested_json_diff(before, after),
                    (Some(before), None) => Some(change_pair(before.clone(), Value::Null)),
                    (None, Some(after)) => Some(change_pair(Value::Null, after.clone())),
                    (None, None) => None,
                };
                if let Some(change) = change {
                    changes.insert(key.clone(), change);
                }
            }
            Some(Value::Object(changes))
        }
        _ => Some(change_pair(before.clone(), after.clone())),
    }
}

fn change_pair(before: Value, after: Value) -> Value {
    json!({"before": before, "after": after})
}

fn empty_json_object() -> Value {
    Value::Object(Map::new())
}
