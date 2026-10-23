use hubuum_client::{blocking::Client, ApiError, Authenticated, SessionInfo};
use reqwest::StatusCode;

use super::HubuumGateway;
use crate::diagnostics::client_call;
use crate::errors::AppError;

impl HubuumGateway {
    pub fn session_info(&self) -> Option<SessionInfo> {
        self.client().session_info()
    }

    pub fn logout_session(&self) -> Result<(), AppError> {
        logout_session_client(self.client().as_ref())
    }
}

pub(crate) fn logout_session_client(client: &Client<Authenticated>) -> Result<(), AppError> {
    if client.session_info().is_none() && client.token().is_ok() {
        return Err(AppError::InvalidOption("auth logout manages rotating sessions; use user token revoke or service-account token revoke for bearer tokens".into()));
    }
    match client_call("client.logout", || client.clone().logout()) {
        Ok(_) | Err(ApiError::SessionExpired) => Ok(()),
        Err(error) if error.is_status(StatusCode::UNAUTHORIZED) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hubuum_client::{
        BaseUrl, Credentials, MockTransport, SessionClientId, StoredSession, Token,
        TransportResponse,
    };
    use hubuum_session::{SessionCache, SessionIdentity, SessionOptions};
    use serde_json::{from_value, json, to_value};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn logout_of_a_replaced_session_preserves_the_new_family() {
        let directory = tempdir().unwrap();
        let base = BaseUrl::new("https://example.invalid").unwrap();
        let identity = SessionIdentity::new(&base, None, "alice").unwrap();
        let cache = SessionCache::open(SessionOptions::new(directory.path(), identity))
            .unwrap()
            .unwrap();
        let transport = MockTransport::default();
        let now = chrono::Utc::now();
        transport.push_response(TransportResponse::json(StatusCode::OK, &json!({
            "session_id":"old-family","token_type":"Bearer","access_token":"old-access","refresh_token":"old-refresh",
            "access_expires_at":now + chrono::Duration::minutes(15),
            "refresh_expires_at":now + chrono::Duration::days(7),
            "absolute_expires_at":now + chrono::Duration::days(30)
        })).unwrap());
        let client = Client::builder_from_url(base.as_str())
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .login_session_with_store(
                Credentials::new("alice", "password"),
                SessionClientId::new("hubuum-cli").unwrap(),
                cache.store(),
            )
            .unwrap();
        let mut replacement =
            to_value(cache.store().lock().unwrap().load().unwrap().unwrap()).unwrap();
        replacement["pair"]["session_id"] = json!("new-family");
        replacement["pair"]["access_token"] = json!("new-access");
        let replacement: StoredSession = from_value(replacement).unwrap();
        cache
            .store()
            .lock()
            .unwrap()
            .save(Some(&replacement))
            .unwrap();
        logout_session_client(&client).unwrap();
        let retained = to_value(cache.store().lock().unwrap().load().unwrap().unwrap()).unwrap();
        assert_eq!(retained["pair"]["session_id"], "new-family");
        assert_eq!(transport.requests().len(), 1);
    }

    #[test]
    fn bearer_logout_requires_an_explicit_token_revoke_command() {
        let transport = MockTransport::default();
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("pat"));
        assert!(logout_session_client(&client).is_err());
        assert!(transport.requests().is_empty());
    }
}
