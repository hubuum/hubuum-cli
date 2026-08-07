use hubuum_client::PrincipalTokenMetadata;

use crate::domain::{
    MeRecord, PrincipalPermissionsRecord, PrincipalTokenDetailsRecord, PrincipalTokenRecord,
};

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
        rows.push(("Token Revision", me.token.revision.to_string()));

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
            "State",
            "Revision",
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
            token_lifecycle(token).to_string(),
            token.revision.to_string(),
        ]
    }
}

impl DetailRenderable for PrincipalTokenDetailsRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let token = self.token();
        let (scope, permissions, resources) = match token.scope.as_ref() {
            Some(scope) => {
                let permissions = scope.permissions().map_or_else(
                    || "<unrestricted>".to_string(),
                    |permissions| {
                        permissions
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                );
                let resources = scope.resources().map_or_else(
                    || "<unrestricted>".to_string(),
                    |_| {
                        self.resolved_resources()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                    },
                );
                ("scoped".to_string(), permissions, resources)
            }
            None => (
                "unscoped".to_string(),
                "<unrestricted>".to_string(),
                "<unrestricted>".to_string(),
            ),
        };

        vec![
            ("Token ID", token.id.to_string()),
            ("Principal ID", token.principal_id.to_string()),
            (
                "Name",
                token.name.clone().unwrap_or_else(|| "<none>".to_string()),
            ),
            (
                "Description",
                token
                    .description
                    .clone()
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            ("Scope", scope),
            ("Permissions", permissions),
            ("Resources", resources),
            ("Issued", token.issued.to_string()),
            (
                "Expires",
                token
                    .expires_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            (
                "Last Used",
                token
                    .last_used_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            (
                "Revoked",
                token
                    .revoked_at
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<none>".to_string()),
            ),
            ("State", token_lifecycle(token).to_string()),
            ("Revision", token.revision.to_string()),
        ]
    }
}

fn token_lifecycle(token: &PrincipalTokenMetadata) -> &'static str {
    if token.revoked_at.is_some() {
        "revoked"
    } else if token.expired {
        "expired"
    } else if token.active {
        "active"
    } else {
        "inactive"
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
    use hubuum_client::{MeResponse, PrincipalTokenMetadata};
    use serde_json::{json, to_value};

    use super::DetailRenderable;
    use crate::domain::{
        MeRecord, PrincipalTokenDetailsRecord, ResolvedTokenResource, TokenResourceParent,
    };

    #[test]
    fn me_details_show_identity_scope() {
        let response: MeResponse = serde_json::from_value(json!({
            "principal": {
                "principal_id": 1,
                "identity_scope": "example-directory",
                "kind": "human",
                "name": "admin",
                "created_at": "2026-07-11T08:00:00Z",
                "updated_at": "2026-07-11T08:00:00Z",
                "revision": 1
            },
            "token": {
                "id": 9,
                "name": null,
                "description": null,
                "scoped": false,
                "scopes": null,
                "issued": "2026-07-11T08:47:51Z",
                "expires_at": null,
                "last_used_at": null,
                "revision": 2
            }
        }))
        .expect("me response should deserialize");

        let rows = MeRecord(response).detail_rows();
        assert!(rows.contains(&("Identity Scope", "example-directory".to_string())));
    }

    #[test]
    fn token_details_preserve_metadata_and_mark_unreachable_objects() {
        let token: PrincipalTokenMetadata = serde_json::from_value(json!({
            "id": 9,
            "principal_id": 2,
            "name": "automation",
            "description": "Ansible token",
            "scope": {
                "permissions": ["ReadObject"],
                "resources": [
                    {"kind": "class", "id": 8},
                    {"kind": "object", "id": 42}
                ]
            },
            "issued": "2026-07-25T08:47:51Z",
            "expires_at": "2026-12-31T23:59:59Z",
            "last_used_at": null,
            "revoked_at": null,
            "active": true,
            "expired": false,
            "revision": 4
        }))
        .expect("token should deserialize");
        let details = PrincipalTokenDetailsRecord::new(
            token,
            vec![
                ResolvedTokenResource::resolved_class(
                    8.into(),
                    "Hosts",
                    TokenResourceParent::new(7.into(), Some("Infrastructure".to_string())),
                ),
                ResolvedTokenResource::unreachable_object(42.into()),
            ],
        );

        let value = to_value(&details).expect("token details should serialize");
        assert_eq!(value["id"], 9);
        assert_eq!(value["principal_id"], 2);
        assert_eq!(value["description"], "Ansible token");
        assert_eq!(value["scope"]["permissions"], json!(["ReadObject"]));
        assert_eq!(value["scoped"], true);
        assert_eq!(value["scopes"], json!(["ReadObject"]));
        assert_eq!(value["resolved_resources"][1]["resolution"], "unreachable");

        let rows = details.detail_rows();
        let resources = rows
            .iter()
            .find(|(key, _)| *key == "Resources")
            .map(|(_, value)| value)
            .expect("resources should be rendered");
        assert!(resources.contains("class 8: Hosts"));
        assert!(resources.contains("object 42 [unreachable]"));
    }
}
