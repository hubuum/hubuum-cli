use std::fmt::Display;

use crate::domain::UserRecord;

use super::{DetailRenderable, TableRenderable};

fn optional_display<T: Display>(value: Option<&T>, fallback: &str) -> String {
    value
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

impl DetailRenderable for UserRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let user = &self.0;
        vec![
            ("ID", user.id.to_string()),
            ("Name", user.name.clone()),
            (
                "Proper Name",
                optional_display(user.proper_name.as_ref(), "<none>"),
            ),
            ("Email", optional_display(user.email.as_ref(), "<none>")),
            ("Identity Scope", user.identity_scope.clone()),
            ("Provider Kind", user.provider_kind.clone()),
            (
                "Provider Managed",
                if user.provider_managed { "yes" } else { "no" }.to_string(),
            ),
            (
                "Last Sync Attempted",
                optional_display(user.last_sync_attempted_at.as_ref(), "<never>"),
            ),
            (
                "Last Sync Succeeded",
                optional_display(user.last_sync_success_at.as_ref(), "<never>"),
            ),
            ("Created", user.created_at.to_string()),
            ("Updated", user.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for UserRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Proper Name",
            "Email",
            "Scope",
            "Provider",
            "Managed",
            "Last Sync",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        let user = &self.0;
        vec![
            user.id.to_string(),
            user.name.clone(),
            optional_display(user.proper_name.as_ref(), ""),
            optional_display(user.email.as_ref(), ""),
            user.identity_scope.clone(),
            user.provider_kind.clone(),
            if user.provider_managed { "yes" } else { "no" }.to_string(),
            optional_display(user.last_sync_success_at.as_ref(), ""),
            user.created_at.to_string(),
            user.updated_at.to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json};

    use super::{DetailRenderable, TableRenderable, UserRecord};

    fn provider_user() -> UserRecord {
        from_value(json!({
            "id": 42,
            "identity_scope": "example-directory",
            "provider_kind": "ldap",
            "provider_managed": true,
            "name": "alice",
            "email": "alice@example.com",
            "proper_name": "Alice Example",
            "created_at": "2026-07-17T10:00:00Z",
            "updated_at": "2026-07-18T11:00:00Z",
            "last_sync_attempted_at": "2026-07-18T10:59:00Z",
            "last_sync_success_at": "2026-07-18T10:59:01Z"
        }))
        .expect("provider user should deserialize")
    }

    #[test]
    fn user_list_includes_provider_identity_context() {
        let user = provider_user();
        let headers = UserRecord::headers();
        let row = user.row();

        assert_eq!(headers.len(), row.len());
        assert_eq!(headers[4..8], ["Scope", "Provider", "Managed", "Last Sync"]);
        assert_eq!(
            row[4..8],
            [
                "example-directory",
                "ldap",
                "yes",
                "2026-07-18T10:59:01+00:00"
            ]
        );
    }

    #[test]
    fn user_details_include_all_server_identity_fields() {
        let rows = provider_user().detail_rows();

        for expected in [
            ("ID", "42"),
            ("Proper Name", "Alice Example"),
            ("Identity Scope", "example-directory"),
            ("Provider Kind", "ldap"),
            ("Provider Managed", "yes"),
            ("Last Sync Attempted", "2026-07-18T10:59:00+00:00"),
            ("Last Sync Succeeded", "2026-07-18T10:59:01+00:00"),
        ] {
            assert!(rows
                .iter()
                .any(|row| row.0 == expected.0 && row.1 == expected.1));
        }
    }
}
