use std::fs::File;
use std::io::{stderr, Read, Write};
use std::path::{Path, PathBuf};

use hubuum_client::{
    blocking::Client, client::sync::ApprovedCredentialOperation, Authenticated,
    CredentialOperation, NewTokenRequest, PrincipalId, RenewTokenRequest, Token, TokenId,
};
use rpassword::prompt_password;
use serde::de::DeserializeOwned;
use serde_json::{to_string, to_value, Value};

use crate::errors::AppError;

#[derive(Clone, Default)]
pub(crate) enum CredentialApprovalSource {
    #[default]
    Unavailable,
    Prompt,
    File(PathBuf),
}

impl CredentialApprovalSource {
    pub(super) fn approve<T: DeserializeOwned>(
        &self,
        client: &Client<Authenticated>,
        operation: CredentialOperation<T>,
        description: &str,
    ) -> Result<ApprovedCredentialOperation<T>, AppError> {
        let password = match self {
            Self::Unavailable => return Err(AppError::CommandExecutionError(
                "The server requires fresh password approval. Run interactively as an unscoped human user, or supply --approval-password-file with that human's current password. A service-account token cannot approve credential changes.".into(),
            )),
            Self::Prompt => {
                eprintln!("Credential operation: {description}");
                prompt_password("Your current password to approve this operation: ")?
            }
            Self::File(path) => read_password_file(path)?,
        };
        if password.is_empty() {
            return Err(AppError::CommandExecutionError(
                "Approval password cannot be empty".into(),
            ));
        }
        // Approval authentication must not enter the REPL's login/replay flow.
        client
            .credential_approvals()
            .approve(password, operation)
            .map_err(|error| {
                AppError::CommandExecutionError(format!("Credential approval failed: {error}"))
            })
    }
}

fn read_password_file(path: &Path) -> Result<String, AppError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::InvalidOption(
            "Approval password source must be a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AppError::InvalidOption(
                "Approval password file must be owner-only; run chmod 600 on it".into(),
            ));
        }
    }
    let mut password = String::new();
    file.take(16_385).read_to_string(&mut password)?;
    if password.len() > 16_384 {
        return Err(AppError::InvalidOption(
            "Approval password file exceeds 16 KiB".into(),
        ));
    }
    // Preserve meaningful spaces; remove only a single conventional line ending.
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    Ok(password)
}

pub(super) fn send_approved<T: DeserializeOwned>(
    approved: ApprovedCredentialOperation<T>,
) -> Result<T, AppError> {
    let id = approved.record().id;
    // Evidence is non-secret and must survive an ambiguous mutation response.
    let mut stderr = stderr().lock();
    writeln!(stderr, "Credential approval ID: {id}")?;
    stderr.flush()?;
    approved.send().map_err(|error| AppError::CommandExecutionError(format!(
        "Approved operation failed (approval ID {id}): {error}. Run auth approval show {id} and inspect the target before retrying; the operation may have committed."
    )))
}

impl super::HubuumGateway {
    pub(crate) fn credential_approval(&self, id: i32) -> Result<Value, AppError> {
        if id <= 0 {
            return Err(AppError::InvalidOption(
                "Credential approval ID must be positive".into(),
            ));
        }
        Ok(to_value(self.client().credential_approvals().get(id)?)?)
    }
    pub(crate) fn set_credential_approval_source(&self, source: CredentialApprovalSource) {
        *self
            .approval_source
            .write()
            .expect("credential approval source lock poisoned") = source;
    }

    pub(super) fn approval_source(&self) -> CredentialApprovalSource {
        self.approval_source
            .read()
            .expect("credential approval source lock poisoned")
            .clone()
    }

    pub(super) fn create_approved_token(
        &self,
        client: &Client<Authenticated>,
        principal: PrincipalId,
        request: NewTokenRequest,
    ) -> Result<Token, AppError> {
        // Token request metadata contains no bearer or password.
        let description = format!(
            "Create token for principal {principal}: {}",
            to_string(&request)?
        );
        send_approved(self.approval_source().approve(
            client,
            CredentialOperation::create_token(principal, request),
            &description,
        )?)
    }

    pub(super) fn renew_approved_token(
        &self,
        client: &Client<Authenticated>,
        principal: PrincipalId,
        token: TokenId,
        request: RenewTokenRequest,
    ) -> Result<Token, AppError> {
        let description = format!(
            "Renew token {token} for principal {principal}: {}",
            to_string(&request)?
        );
        send_approved(self.approval_source().approve(
            client,
            CredentialOperation::renew_token(principal, token, request),
            &description,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{HubuumGateway, NewTokenInput};
    use hubuum_client::{MockTransport, TransportResponse};
    use reqwest::StatusCode;
    use serde_json::{from_slice, json, Value};
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    fn fixture(transport: &MockTransport) -> (HubuumGateway, NamedTempFile) {
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("human-bearer"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "actor-password").unwrap();
        gateway.set_credential_approval_source(CredentialApprovalSource::File(
            file.path().to_path_buf(),
        ));
        (gateway, file)
    }
    fn user_response(transport: &MockTransport) {
        transport.push_response(TransportResponse::json(StatusCode::OK, &json!([{
            "id":7,"name":"alice","email":null,"proper_name":null,"created_at":"2026-09-22T00:00:00Z","updated_at":"2026-09-22T00:00:00Z","revision":1
        }])).unwrap());
    }
    fn rejection(transport: &MockTransport, reason: &str) {
        transport.push_response(TransportResponse::json(StatusCode::FORBIDDEN, &json!({"error":"Forbidden","message":"Credential change rejected","reason":reason})).unwrap());
    }
    fn approval(transport: &MockTransport) {
        transport.push_response(TransportResponse::json(StatusCode::CREATED, &json!({
            "approval":format!("hca1.{}", "a".repeat(64)),
            "record":{"id":11,"actor_id":7,"token_id":18,"operation":"create_token","target_id":7,"restore_job_id":null,"authenticated_at":"2026-09-22T00:00:00Z","expires_at":"2026-09-22T00:02:00Z","consumed_at":null,"invalidated_at":null},
            "token_expires_at":"2027-01-01T00:00:00.123456"
        })).unwrap());
    }
    fn input() -> NewTokenInput {
        NewTokenInput {
            name: Some("inventory".into()),
            description: None,
            expires_at: None,
            scopes: vec!["ReadObject".into()],
        }
    }

    #[test]
    fn approval_is_bound_to_original_bearer_payload_and_server_expiry() {
        let transport = MockTransport::default();
        user_response(&transport);
        rejection(&transport, "reauthentication_required");
        approval(&transport);
        transport.push_response(
            TransportResponse::json(
                StatusCode::CREATED,
                &json!({"token":"issued-secret","expires_at":"2027-01-01T00:00:00.123456"}),
            )
            .unwrap(),
        );
        let (gateway, _password_file) = fixture(&transport);
        let token = gateway.user_token_create("alice", input()).unwrap();
        assert_eq!(token.token(), "issued-secret");
        let requests = transport.requests();
        assert_eq!(requests.len(), 4);
        let approval: Value = from_slice(requests[2].body()).unwrap();
        assert_eq!(approval["password"], "actor-password");
        assert_eq!(approval["operation"]["token"]["name"], "inventory");
        let body: Value = from_slice(requests[3].body()).unwrap();
        assert_eq!(body["expires_at"], "2027-01-01T00:00:00.123456");
        assert_eq!(body["scope"], approval["operation"]["token"]["scope"]);
        assert!(requests[3].headers["x-hubuum-credential-approval"].is_sensitive());
        for request in &requests {
            assert_eq!(request.headers["authorization"], "Bearer human-bearer");
        }
    }

    #[test]
    fn ordinary_permission_failures_do_not_request_approval() {
        let transport = MockTransport::default();
        user_response(&transport);
        rejection(&transport, "permission_denied");
        let (gateway, _password_file) = fixture(&transport);
        assert!(gateway.user_token_create("alice", input()).is_err());
        assert_eq!(transport.requests().len(), 2);
    }

    #[test]
    fn failed_approved_send_preserves_evidence_without_replay_or_secrets() {
        let transport = MockTransport::default();
        user_response(&transport);
        rejection(&transport, "reauthentication_required");
        approval(&transport);
        rejection(&transport, "reauthentication_required");
        let (gateway, _password_file) = fixture(&transport);
        let error = gateway.user_token_create("alice", input()).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("approval ID 11"));
        assert!(!message.contains("actor-password"));
        assert!(!message.contains("hca1."));
        assert!(matches!(error, AppError::CommandExecutionError(_)));
        assert_eq!(transport.requests().len(), 4);
    }

    #[test]
    fn password_file_preserves_spaces_and_rejects_empty_or_insecure_sources() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, " password with spaces \r\n").unwrap();
        assert_eq!(
            read_password_file(file.path()).unwrap(),
            " password with spaces "
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_password_file(file.path()).is_err());
        }
    }
}
