use hubuum_client::{
    ClassId, CollectionId, MeResponse, ObjectId, PrincipalCollectionPermissions,
    PrincipalTokenMetadata, ServiceAccount, Token,
};
use serde::{Serialize, Serializer};
use serde_json::to_value;
use std::fmt::{Debug, Display, Formatter};

transparent_record!(MeRecord, MeResponse);
transparent_record!(PrincipalTokenRecord, PrincipalTokenMetadata);
transparent_record!(PrincipalPermissionsRecord, PrincipalCollectionPermissions);
transparent_record!(ServiceAccountRecord, ServiceAccount);

pub struct IssuedTokenRecord {
    token: String,
    expires_at: Option<String>,
}

impl IssuedTokenRecord {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn expires_at(&self) -> Option<&str> {
        self.expires_at.as_deref()
    }
}

impl From<Token> for IssuedTokenRecord {
    fn from(token: Token) -> Self {
        let expires_at = token.expires_at().map(ToString::to_string);
        Self {
            token: token.into_inner(),
            expires_at,
        }
    }
}

impl Debug for IssuedTokenRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedTokenRecord")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct PrincipalTokenDetailsRecord {
    token: PrincipalTokenMetadata,
    resolved_resources: Vec<ResolvedTokenResource>,
}

impl PrincipalTokenDetailsRecord {
    pub(crate) fn new(
        token: PrincipalTokenMetadata,
        resolved_resources: Vec<ResolvedTokenResource>,
    ) -> Self {
        Self {
            token,
            resolved_resources,
        }
    }

    pub(crate) const fn token(&self) -> &PrincipalTokenMetadata {
        &self.token
    }

    pub(crate) fn resolved_resources(&self) -> &[ResolvedTokenResource] {
        &self.resolved_resources
    }
}

impl Serialize for PrincipalTokenDetailsRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = to_value(&self.token).map_err(serde::ser::Error::custom)?;
        let object = value.as_object_mut().ok_or_else(|| {
            serde::ser::Error::custom("principal token metadata must serialize as an object")
        })?;
        object.insert(
            "scoped".to_string(),
            to_value(self.token.scoped).map_err(serde::ser::Error::custom)?,
        );
        object.insert(
            "scopes".to_string(),
            to_value(&self.token.scopes).map_err(serde::ser::Error::custom)?,
        );
        object.insert(
            "resolved_resources".to_string(),
            to_value(&self.resolved_resources).map_err(serde::ser::Error::custom)?,
        );
        value.serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TokenResourceResolution {
    Resolved,
    Unresolved,
    Unreachable,
}

impl Display for TokenResourceResolution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolved => formatter.write_str("resolved"),
            Self::Unresolved => formatter.write_str("unresolved"),
            Self::Unreachable => formatter.write_str("unreachable"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ResolvedTokenResource {
    Collection {
        id: CollectionId,
        name: Option<String>,
        resolution: TokenResourceResolution,
    },
    Class {
        id: ClassId,
        name: Option<String>,
        collection_id: Option<CollectionId>,
        collection_name: Option<String>,
        resolution: TokenResourceResolution,
    },
    Object {
        id: ObjectId,
        name: Option<String>,
        class_id: Option<ClassId>,
        class_name: Option<String>,
        collection_id: Option<CollectionId>,
        collection_name: Option<String>,
        resolution: TokenResourceResolution,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenResourceParent<Id> {
    id: Id,
    name: Option<String>,
}

impl<Id> TokenResourceParent<Id> {
    pub(crate) fn new(id: Id, name: Option<String>) -> Self {
        Self { id, name }
    }
}

impl ResolvedTokenResource {
    pub(crate) fn resolved_collection(id: CollectionId, name: impl Into<String>) -> Self {
        Self::Collection {
            id,
            name: Some(name.into()),
            resolution: TokenResourceResolution::Resolved,
        }
    }

    pub(crate) const fn unresolved_collection(id: CollectionId) -> Self {
        Self::Collection {
            id,
            name: None,
            resolution: TokenResourceResolution::Unresolved,
        }
    }

    pub(crate) fn resolved_class(
        id: ClassId,
        name: impl Into<String>,
        collection: TokenResourceParent<CollectionId>,
    ) -> Self {
        Self::Class {
            id,
            name: Some(name.into()),
            collection_id: Some(collection.id),
            collection_name: collection.name,
            resolution: TokenResourceResolution::Resolved,
        }
    }

    pub(crate) const fn unresolved_class(id: ClassId) -> Self {
        Self::Class {
            id,
            name: None,
            collection_id: None,
            collection_name: None,
            resolution: TokenResourceResolution::Unresolved,
        }
    }

    pub(crate) fn resolved_class_without_collection(id: ClassId, name: impl Into<String>) -> Self {
        Self::Class {
            id,
            name: Some(name.into()),
            collection_id: None,
            collection_name: None,
            resolution: TokenResourceResolution::Resolved,
        }
    }

    pub(crate) fn resolved_object(
        id: ObjectId,
        name: impl Into<String>,
        class: TokenResourceParent<ClassId>,
        collection: TokenResourceParent<CollectionId>,
    ) -> Self {
        Self::Object {
            id,
            name: Some(name.into()),
            class_id: Some(class.id),
            class_name: class.name,
            collection_id: Some(collection.id),
            collection_name: collection.name,
            resolution: TokenResourceResolution::Resolved,
        }
    }

    pub(crate) const fn unreachable_object(id: ObjectId) -> Self {
        Self::Object {
            id,
            name: None,
            class_id: None,
            class_name: None,
            collection_id: None,
            collection_name: None,
            resolution: TokenResourceResolution::Unreachable,
        }
    }
}

impl Display for ResolvedTokenResource {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Collection {
                id,
                name,
                resolution,
            } => {
                write_resolved_reference(formatter, "collection", id, name.as_deref(), *resolution)
            }
            Self::Class {
                id,
                name,
                collection_id,
                collection_name,
                resolution,
            } => {
                write_resolved_reference(formatter, "class", id, name.as_deref(), *resolution)?;
                write_parent_reference(
                    formatter,
                    "collection",
                    collection_id.as_ref(),
                    collection_name.as_deref(),
                )
            }
            Self::Object {
                id,
                name,
                class_id,
                class_name,
                collection_id,
                collection_name,
                resolution,
            } => {
                write_resolved_reference(formatter, "object", id, name.as_deref(), *resolution)?;
                write_parent_reference(
                    formatter,
                    "class",
                    class_id.as_ref(),
                    class_name.as_deref(),
                )?;
                write_parent_reference(
                    formatter,
                    "collection",
                    collection_id.as_ref(),
                    collection_name.as_deref(),
                )
            }
        }
    }
}

fn write_resolved_reference<T: Display>(
    formatter: &mut Formatter<'_>,
    kind: &str,
    id: T,
    name: Option<&str>,
    resolution: TokenResourceResolution,
) -> std::fmt::Result {
    write!(formatter, "{kind} {id}")?;
    if let Some(name) = name {
        write!(formatter, ": {name}")?;
    }
    if resolution != TokenResourceResolution::Resolved {
        write!(formatter, " [{resolution}]")?;
    }
    Ok(())
}

fn write_parent_reference<T: Display>(
    formatter: &mut Formatter<'_>,
    kind: &str,
    id: Option<&T>,
    name: Option<&str>,
) -> std::fmt::Result {
    let Some(id) = id else {
        return Ok(());
    };
    write!(formatter, " ({kind} {id}")?;
    if let Some(name) = name {
        write!(formatter, ": {name}")?;
    }
    formatter.write_str(")")
}

#[cfg(test)]
mod tests {
    use hubuum_client::Token;
    use serde_json::{from_value, json};

    use super::IssuedTokenRecord;

    #[test]
    fn issued_token_preserves_expiry_and_redacts_debug_output() {
        let token: Token = from_value(json!({
            "token": "issued-secret",
            "expires_at": "2026-07-27T05:17:17Z"
        }))
        .expect("issued token should deserialize");

        let issued = IssuedTokenRecord::from(token);

        assert_eq!(issued.token(), "issued-secret");
        assert_eq!(issued.expires_at(), Some("2026-07-27T05:17:17+00:00"));
        assert!(!format!("{issued:?}").contains("issued-secret"));
    }
}
