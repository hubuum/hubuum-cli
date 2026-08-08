use serde::{Deserialize, Serialize};
use serde_json::to_value;

use hubuum_client::{PrincipalSettingsPatchDocument, PrincipalSettingsPatchOperation};

use crate::config::UserPreferences;
use crate::errors::AppError;

use super::HubuumGateway;

const SETTINGS_NAMESPACE: &str = "hubuum-cli";
const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct StoredUserPreferences {
    version: u32,
    preferences: UserPreferences,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerUserPreferences {
    namespace: String,
    revision: i64,
    version: u32,
    preferences: UserPreferences,
}

impl ServerUserPreferences {
    fn new(revision: i64, stored: StoredUserPreferences) -> Self {
        Self {
            namespace: SETTINGS_NAMESPACE.to_string(),
            revision,
            version: stored.version,
            preferences: stored.preferences,
        }
    }

    pub fn into_preferences(self) -> UserPreferences {
        self.preferences
    }
}

impl HubuumGateway {
    pub fn load_user_preferences(&self) -> Result<UserPreferences, AppError> {
        Ok(self.server_user_preferences()?.into_preferences())
    }

    pub fn server_user_preferences(&self) -> Result<ServerUserPreferences, AppError> {
        let settings = self.client().settings().get()?;
        let stored = settings.settings.get(SETTINGS_NAMESPACE).ok_or_else(|| {
            AppError::EntityNotFound(format!(
                "no settings are stored under the '{SETTINGS_NAMESPACE}' namespace"
            ))
        })?;
        let stored = decode_stored_preferences(stored.clone())?;
        Ok(ServerUserPreferences::new(settings.revision.get(), stored))
    }

    pub fn store_user_preferences(
        &self,
        preferences: &UserPreferences,
    ) -> Result<UserPreferences, AppError> {
        let stored = to_value(StoredUserPreferences {
            version: SETTINGS_VERSION,
            preferences: preferences.clone(),
        })?;
        let patch = PrincipalSettingsPatchDocument::new([PrincipalSettingsPatchOperation::Add {
            path: format!("/{SETTINGS_NAMESPACE}"),
            value: stored,
        }])?;
        let updated = self.client().settings().json_patch(&patch)?;
        let stored = updated.settings.get(SETTINGS_NAMESPACE).ok_or_else(|| {
            AppError::GeneralConfigError(
                "server response omitted the stored Hubuum CLI settings".to_string(),
            )
        })?;
        decode_preferences(stored.clone())
    }
}

fn decode_preferences(value: serde_json::Value) -> Result<UserPreferences, AppError> {
    Ok(decode_stored_preferences(value)?.preferences)
}

fn decode_stored_preferences(value: serde_json::Value) -> Result<StoredUserPreferences, AppError> {
    let stored: StoredUserPreferences = serde_json::from_value(value)?;
    if stored.version != SETTINGS_VERSION {
        return Err(AppError::GeneralConfigError(format!(
            "unsupported Hubuum CLI settings version {}; expected {SETTINGS_VERSION}",
            stored.version
        )));
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::{header::CONTENT_TYPE, Method, StatusCode};
    use serde_json::json;

    use super::{decode_preferences, HubuumGateway, SETTINGS_NAMESPACE, SETTINGS_VERSION};
    use crate::config::{AppConfig, UserPreferences};
    use crate::domain::ComputedFieldSet;

    #[test]
    fn stored_preferences_round_trip_without_server_credentials() {
        let mut config = AppConfig {
            aliases: serde_json::from_value(json!({
                "outdated-kernels": {
                    "command": "object list --class Hosts | C",
                    "description": "Find hosts running an outdated kernel"
                }
            }))
            .expect("command aliases should deserialize"),
            ..AppConfig::default()
        };
        config.output.object_class_computed_fields.insert(
            "Hosts".to_string(),
            ComputedFieldSet::from_values(&["S:load".to_string()])
                .expect("computed fields should parse"),
        );
        let preferences = UserPreferences::from_config(&config);
        let encoded = json!({
            "version": SETTINGS_VERSION,
            "preferences": preferences,
        });
        let decoded = decode_preferences(encoded).expect("preferences should decode");
        assert_eq!(decoded.output.theme, config.output.theme);
        assert_eq!(decoded.relations.max_depth, config.relations.max_depth);
        assert_eq!(
            decoded.aliases.get("outdated-kernels"),
            config.aliases.get("outdated-kernels")
        );
        assert_eq!(
            decoded.aliases.description("outdated-kernels"),
            config.aliases.description("outdated-kernels")
        );
        assert_eq!(
            decoded.output.object_class_computed_fields["Hosts"],
            config.output.object_class_computed_fields["Hosts"]
        );
    }

    #[test]
    fn stored_preferences_without_computed_defaults_remain_compatible() {
        let mut encoded = json!({
            "version": SETTINGS_VERSION,
            "preferences": UserPreferences::from_config(&AppConfig::default()),
        });
        encoded["preferences"]["output"]
            .as_object_mut()
            .expect("output preferences should be an object")
            .remove("object_class_computed_fields");
        encoded["preferences"]
            .as_object_mut()
            .expect("preferences should be an object")
            .remove("aliases");

        let decoded = decode_preferences(encoded).expect("older preferences should decode");

        assert!(decoded.output.object_class_computed_fields.is_empty());
        assert!(decoded.aliases.is_empty());
    }

    #[test]
    fn stored_preferences_accept_legacy_meta_column_name() {
        let mut config = AppConfig::default();
        config.output.object_list_class_aliases.insert(
            "Hosts".to_string(),
            HashMap::from([(
                "os_version".to_string(),
                vec!["data.os.version".to_string()],
            )]),
        );
        let preferences = UserPreferences::from_config(&config);
        let mut encoded = json!({
            "version": SETTINGS_VERSION,
            "preferences": preferences,
        });
        let output = encoded["preferences"]["output"]
            .as_object_mut()
            .expect("output preferences should be an object");
        let aliases = output
            .remove("object_list_class_aliases")
            .expect("new display alias key should exist");
        output.insert("object_list_class_meta".to_string(), aliases);

        let decoded = decode_preferences(encoded).expect("legacy preferences should decode");

        assert!(decoded
            .output
            .object_list_class_aliases
            .get("Hosts")
            .is_some_and(|aliases| aliases.contains_key("os_version")));
    }

    #[test]
    fn rejects_unknown_settings_versions() {
        let preferences = UserPreferences::from_config(&AppConfig::default());
        let error = decode_preferences(json!({
            "version": SETTINGS_VERSION + 1,
            "preferences": preferences,
        }))
        .expect_err("unknown version should fail");
        assert!(error
            .to_string()
            .contains("unsupported Hubuum CLI settings version"));
    }

    #[test]
    fn storing_preferences_atomically_replaces_only_the_cli_namespace() {
        let preferences = UserPreferences::from_config(&AppConfig::default());
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!({
                    "revision": 2,
                    "settings": {
                        "another-client": {"keep": true},
                        "hubuum-cli": {
                            "version": SETTINGS_VERSION,
                            "preferences": preferences.clone()
                        }
                    }
                }),
            )
            .expect("settings response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let stored = gateway
            .store_user_preferences(&preferences)
            .expect("preferences should be stored");

        assert_eq!(stored.output.theme, preferences.output.theme);
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::PATCH);
        assert_eq!(requests[0].url.path(), "/api/v1/iam/me/settings");
        assert_eq!(
            requests[0]
                .headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json-patch+json")
        );
        let body: serde_json::Value =
            serde_json::from_slice(requests[0].body()).expect("patch body should be JSON");
        assert_eq!(body[0]["op"], "add");
        assert_eq!(body[0]["path"], "/hubuum-cli");
        assert_eq!(body[0]["value"]["version"], SETTINGS_VERSION);
        assert!(body[0]["value"].get("another-client").is_none());
    }

    #[test]
    fn server_preferences_expose_the_namespace_and_revision_without_mutation() {
        let preferences = UserPreferences::from_config(&AppConfig::default());
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(
                StatusCode::OK,
                &json!({
                    "revision": 7,
                    "settings": {
                        "another-client": {"private": true},
                        "hubuum-cli": {
                            "version": SETTINGS_VERSION,
                            "preferences": preferences
                        }
                    }
                }),
            )
            .expect("settings response should serialize"),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .expect("base URL should parse")
            .with_transport(Arc::new(transport.clone()))
            .build()
            .expect("client should build")
            .authenticate(Token::new("secret"));
        let gateway = HubuumGateway::new(Arc::new(client));

        let snapshot = gateway
            .server_user_preferences()
            .expect("server preferences should load");
        let snapshot = serde_json::to_value(snapshot).expect("snapshot should serialize");

        assert_eq!(snapshot["namespace"], SETTINGS_NAMESPACE);
        assert_eq!(snapshot["revision"], 7);
        assert_eq!(snapshot["version"], SETTINGS_VERSION);
        assert!(snapshot.get("another-client").is_none());
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::GET);
        assert_eq!(requests[0].url.path(), "/api/v1/iam/me/settings");
    }
}
