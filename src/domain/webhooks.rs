use reqwest::Url;
use serde_json::{json, Value};

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebhookTarget {
    Slack,
    Mattermost,
    Discord,
}

impl WebhookTarget {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "slack" => Ok(Self::Slack),
            "mattermost" => Ok(Self::Mattermost),
            "discord" => Ok(Self::Discord),
            _ => Err(AppError::InvalidOption(
                "--target must be slack, mattermost, or discord".to_string(),
            )),
        }
    }

    pub(crate) fn config(self, url_secret: WebhookUrlSecret) -> Value {
        let mut config = self.base_config();
        config["url_secret_ref"] = Value::String(url_secret.as_str().to_string());
        config
    }

    pub(crate) fn config_with_url(self, url: &str) -> Result<Value, AppError> {
        let url = url.trim();
        let valid = Url::parse(url).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        });
        if !valid {
            return Err(AppError::InvalidOption(
                "Destination must be an HTTPS URL without embedded credentials".into(),
            ));
        }
        let mut config = self.base_config();
        config["destination_url"] = Value::String(url.to_string());
        Ok(config)
    }

    fn base_config(self) -> Value {
        let mut response = json!({
            "success_statuses": [200],
            "rate_limit": true,
            "retry_statuses": [408, 500, 502, 503, 504]
        });
        let template = match self {
            Self::Slack | Self::Mattermost => {
                response["body"] = json!({"kind": "text_equals", "value": "ok"});
                r#"{"text": {{ (test_marker ~ 'Hubuum: ' ~ summary) | tojson }}}"#
            }
            Self::Discord => {
                r#"{"content": {{ (test_marker ~ 'Hubuum: ' ~ summary)[:1900] | tojson }}, "allowed_mentions": {"parse": []}}"#
            }
        };
        json!({
            "body_template": template,
            "response": response,
        })
    }
}

/// An alias resolved by the server's secret source, never the secret URL itself.
pub(crate) struct WebhookUrlSecret(String);

impl WebhookUrlSecret {
    pub(crate) fn new(value: String) -> Result<Self, AppError> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(AppError::InvalidOption(
                "--url-secret-ref must be a server secret alias containing 1-128 ASCII letters, numbers, underscores, or hyphens; store the full HTTPS webhook URL in the server's secret source".to_string(),
            ));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::WebhookUrlSecret;

    #[test]
    fn url_secrets_validate_aliases_without_echoing_credentials() {
        for value in [
            "",
            "https://example.com/hooks/secret",
            "ops chat",
            "å",
            &"a".repeat(129),
        ] {
            let error = WebhookUrlSecret::new(value.to_string())
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("server secret alias"));
            assert!(!error.contains("hooks/secret"));
        }
        for value in ["ops_chat-1", &"a".repeat(128)] {
            assert!(WebhookUrlSecret::new(value.to_string()).is_ok());
        }
    }
}
