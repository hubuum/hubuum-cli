use serde::{Deserialize, Serialize};
use serde_json::to_value;

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

impl HubuumGateway {
    pub fn load_user_preferences(&self) -> Result<UserPreferences, AppError> {
        let settings = self.client.settings().get()?;
        let stored = settings.get(SETTINGS_NAMESPACE).ok_or_else(|| {
            AppError::EntityNotFound(format!(
                "no settings are stored under the '{SETTINGS_NAMESPACE}' namespace"
            ))
        })?;
        decode_preferences(stored.clone())
    }

    pub fn store_user_preferences(
        &self,
        preferences: &UserPreferences,
    ) -> Result<UserPreferences, AppError> {
        let mut settings = self.client.settings().get()?;
        settings.insert(
            SETTINGS_NAMESPACE,
            to_value(StoredUserPreferences {
                version: SETTINGS_VERSION,
                preferences: preferences.clone(),
            })?,
        );
        let updated = self.client.settings().replace(&settings)?;
        let stored = updated.get(SETTINGS_NAMESPACE).ok_or_else(|| {
            AppError::GeneralConfigError(
                "server response omitted the stored Hubuum CLI settings".to_string(),
            )
        })?;
        decode_preferences(stored.clone())
    }
}

fn decode_preferences(value: serde_json::Value) -> Result<UserPreferences, AppError> {
    let stored: StoredUserPreferences = serde_json::from_value(value)?;
    if stored.version != SETTINGS_VERSION {
        return Err(AppError::GeneralConfigError(format!(
            "unsupported Hubuum CLI settings version {}; expected {SETTINGS_VERSION}",
            stored.version
        )));
    }
    Ok(stored.preferences)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{decode_preferences, SETTINGS_VERSION};
    use crate::config::{AppConfig, UserPreferences};
    use crate::domain::ComputedFieldSet;

    #[test]
    fn stored_preferences_round_trip_without_server_credentials() {
        let mut config = AppConfig::default();
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

        let decoded = decode_preferences(encoded).expect("older preferences should decode");

        assert!(decoded.output.object_class_computed_fields.is_empty());
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
}
