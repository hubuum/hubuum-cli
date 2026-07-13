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
    use serde_json::json;

    use super::{decode_preferences, SETTINGS_VERSION};
    use crate::config::{AppConfig, UserPreferences};

    #[test]
    fn stored_preferences_round_trip_without_server_credentials() {
        let config = AppConfig::default();
        let preferences = UserPreferences::from_config(&config);
        let encoded = json!({
            "version": SETTINGS_VERSION,
            "preferences": preferences,
        });
        let decoded = decode_preferences(encoded).expect("preferences should decode");
        assert_eq!(decoded.output.theme, config.output.theme);
        assert_eq!(decoded.relations.max_depth, config.relations.max_depth);
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
