use crate::domain::{MeRecord, PrincipalPermissionsRecord, PrincipalTokenRecord};

use super::{DetailRenderable, TableRenderable};

impl DetailRenderable for MeRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let me = &self.0;
        let mut rows = vec![
            ("Principal ID", me.principal.principal_id.to_string()),
            ("Kind", me.principal.kind.clone()),
            ("Name", me.principal.name.clone()),
            ("Identity Scope", me.principal.identity_scope.clone()),
            ("Token ID", me.token.id.to_string()),
            (
                "Token Name",
                me.token
                    .name
                    .clone()
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            ("Token Scoped", me.token.scoped.to_string()),
        ];

        if let Some(scopes) = &me.token.scopes {
            rows.push((
                "Token Scopes",
                scopes
                    .iter()
                    .map(|s| format!("{:?}", s))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        rows.push(("Token Issued", me.token.issued.to_string()));

        if let Some(expires_at) = &me.token.expires_at {
            rows.push(("Token Expires", expires_at.to_string()));
        }

        if let Some(last_used) = &me.token.last_used_at {
            rows.push(("Token Last Used", last_used.to_string()));
        }

        rows
    }
}

impl TableRenderable for PrincipalTokenRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Scoped",
            "Issued",
            "Expires",
            "Last Used",
            "Revoked",
        ]
    }

    fn row(&self) -> Vec<String> {
        let token = &self.0;
        vec![
            token.id.to_string(),
            token.name.clone().unwrap_or_default(),
            token.scoped.to_string(),
            token.issued.to_string(),
            token
                .expires_at
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_default(),
            token
                .last_used_at
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_default(),
            token
                .revoked_at
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_default(),
        ]
    }
}

impl DetailRenderable for PrincipalPermissionsRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let perms = &self.0;
        let groups_str = perms
            .grants
            .iter()
            .map(|g| format!("{} ({})", g.groupname, g.group_id))
            .collect::<Vec<_>>()
            .join(", ");

        let all_perms: Vec<String> = perms
            .grants
            .iter()
            .flat_map(|g| g.permissions.iter().map(|p| format!("{:?}", p)))
            .collect();

        let unique_perms: Vec<String> = {
            let mut set = HashSet::new();
            for p in all_perms {
                set.insert(p);
            }
            let mut vec: Vec<_> = set.into_iter().collect();
            vec.sort();
            vec
        };

        vec![
            ("Collection ID", perms.collection_id.to_string()),
            ("Collection", perms.collection_name.clone()),
            ("Groups", groups_str),
            ("Permissions", unique_perms.join(", ")),
        ]
    }
}

impl TableRenderable for PrincipalPermissionsRecord {
    fn headers() -> Vec<&'static str> {
        vec!["Collection ID", "Collection", "Groups", "Permissions"]
    }

    fn row(&self) -> Vec<String> {
        let perms = &self.0;
        let groups_str = perms
            .grants
            .iter()
            .map(|g| g.groupname.clone())
            .collect::<Vec<_>>()
            .join(", ");

        let all_perms: Vec<String> = perms
            .grants
            .iter()
            .flat_map(|g| g.permissions.iter().map(|p| format!("{:?}", p)))
            .collect();

        let unique_perms: Vec<String> = {
            let mut set = HashSet::new();
            for p in all_perms {
                set.insert(p);
            }
            let mut vec: Vec<_> = set.into_iter().collect();
            vec.sort();
            vec
        };

        vec![
            perms.collection_id.to_string(),
            perms.collection_name.clone(),
            groups_str,
            unique_perms.join(", "),
        ]
    }
}
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use hubuum_client::MeResponse;
    use serde_json::json;

    use super::DetailRenderable;
    use crate::domain::MeRecord;

    #[test]
    fn me_details_show_identity_scope() {
        let response: MeResponse = serde_json::from_value(json!({
            "principal": {
                "principal_id": 1,
                "identity_scope": "example-directory",
                "kind": "human",
                "name": "admin",
                "created_at": null,
                "updated_at": null
            },
            "token": {
                "id": 9,
                "name": null,
                "description": null,
                "scoped": false,
                "scopes": null,
                "issued": "2026-07-11T08:47:51Z",
                "expires_at": null,
                "last_used_at": null
            }
        }))
        .expect("me response should deserialize");

        let rows = MeRecord(response).detail_rows();
        assert!(rows.contains(&("Identity Scope", "example-directory".to_string())));
    }
}
