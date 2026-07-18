use std::fmt::Display;

use crate::domain::{GroupRecord, PrincipalMemberRecord};

use super::{DetailRenderable, TableRenderable};

fn optional_display<T: Display>(value: Option<&T>, fallback: &str) -> String {
    value
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

impl DetailRenderable for GroupRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let group = &self.0;
        vec![
            ("ID", group.id.to_string()),
            ("Name", group.groupname.clone()),
            ("Description", group.description.clone()),
            ("Identity Scope", group.identity_scope.clone()),
            ("Managed By", group.managed_by.clone()),
            (
                "Provider Managed",
                if group.is_provider_managed() {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
            ),
            (
                "External Key",
                optional_display(group.external_key.as_ref(), "<none>"),
            ),
            (
                "Last Sync Attempted",
                optional_display(group.last_sync_attempted_at.as_ref(), "<never>"),
            ),
            (
                "Last Sync Succeeded",
                optional_display(group.last_sync_success_at.as_ref(), "<never>"),
            ),
            ("Created", group.created_at.to_string()),
            ("Updated", group.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for GroupRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Scope",
            "Provider",
            "Managed",
            "External Key",
            "Last Sync",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        let group = &self.0;
        vec![
            group.id.to_string(),
            group.groupname.clone(),
            group.description.clone(),
            group.identity_scope.clone(),
            group.managed_by.clone(),
            if group.is_provider_managed() {
                "yes"
            } else {
                "no"
            }
            .to_string(),
            optional_display(group.external_key.as_ref(), ""),
            optional_display(group.last_sync_success_at.as_ref(), ""),
            group.created_at.to_string(),
            group.updated_at.to_string(),
        ]
    }
}

impl TableRenderable for PrincipalMemberRecord {
    fn headers() -> Vec<&'static str> {
        vec!["Principal ID", "Kind", "Name"]
    }

    fn row(&self) -> Vec<String> {
        let member = &self.0;
        vec![
            member.principal_id.to_string(),
            member.kind.clone(),
            member.name.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json};

    use super::{DetailRenderable, GroupRecord, TableRenderable};

    fn provider_group() -> GroupRecord {
        from_value(json!({
            "id": 17,
            "identity_scope": "example-directory",
            "groupname": "operators",
            "description": "Infrastructure operators",
            "managed_by": "ldap",
            "external_key": "cn=operators,ou=groups,dc=example,dc=com",
            "last_sync_attempted_at": "2026-07-18T10:59:00Z",
            "last_sync_success_at": "2026-07-18T10:59:01Z",
            "created_at": "2026-07-17T10:00:00Z",
            "updated_at": "2026-07-18T11:00:00Z"
        }))
        .expect("provider group should deserialize")
    }

    #[test]
    fn group_list_includes_provider_identity_context() {
        let group = provider_group();
        let headers = GroupRecord::headers();
        let row = group.row();

        assert_eq!(headers.len(), row.len());
        assert_eq!(
            headers[3..8],
            ["Scope", "Provider", "Managed", "External Key", "Last Sync"]
        );
        assert_eq!(
            row[3..8],
            [
                "example-directory",
                "ldap",
                "yes",
                "cn=operators,ou=groups,dc=example,dc=com",
                "2026-07-18T10:59:01+00:00"
            ]
        );
    }

    #[test]
    fn group_details_include_all_server_identity_fields() {
        let rows = provider_group().detail_rows();

        for expected in [
            ("ID", "17"),
            ("Identity Scope", "example-directory"),
            ("Managed By", "ldap"),
            ("Provider Managed", "yes"),
            ("External Key", "cn=operators,ou=groups,dc=example,dc=com"),
            ("Last Sync Attempted", "2026-07-18T10:59:00+00:00"),
            ("Last Sync Succeeded", "2026-07-18T10:59:01+00:00"),
        ] {
            assert!(rows
                .iter()
                .any(|row| row.0 == expected.0 && row.1 == expected.1));
        }
    }
}
