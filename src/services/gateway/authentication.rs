//! Runtime provider use cases. The SDK owns every Hubuum authentication request.
use std::fs::File;
use std::io::{self, Read};

use hubuum_client::{
    ApiError, AuthenticationInteractionRequest, AuthenticationProviderId,
    AuthenticationProviderKind, AuthenticationProviderUpdate, CredentialOperation,
    LdapCandidateVerificationRequest, ProviderConfigurationInput, ProviderCredentialInput,
    ResourceRevision, SecurityCandidateReport, SecurityCandidateRequest, SecurityCandidateState,
    SecurityConfigurationSource, SessionClientId,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{credential_approvals::send_approved, HubuumGateway};
use crate::authentication::InteractionDriver;
use crate::diagnostics::client_call;
use crate::errors::AppError;

const MAX_INPUT: u64 = 1024 * 1024;

fn invalid_input() -> AppError {
    AppError::InvalidOption("Invalid provider candidate; use the authentication configuration schema and protected input file".into())
}

fn enabled() -> bool {
    true
}
fn runtime() -> SecurityConfigurationSource {
    SecurityConfigurationSource::Runtime
}

#[derive(Deserialize)]
#[serde(tag = "group", rename_all = "snake_case", deny_unknown_fields)]
enum CandidateFile {
    Authentication {
        expected_revision: ResourceRevision,
        providers: Vec<ProviderFile>,
        #[serde(default)]
        release_ownership: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderFile {
    provider_id: Option<AuthenticationProviderId>,
    name: String,
    kind: AuthenticationProviderKind,
    #[serde(default = "runtime")]
    source: SecurityConfigurationSource,
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default)]
    adopt_deployment: bool,
    configuration: Option<Map<String, Value>>,
    verification: Option<DirectoryCredentials>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryCredentials {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum CredentialFile {
    Keep,
    Clear,
    Reference { alias: String },
    Replace { value: String },
}

impl CredentialFile {
    fn into_input(self) -> Result<ProviderCredentialInput, ApiError> {
        Ok(match self {
            Self::Keep => ProviderCredentialInput::keep(),
            Self::Clear => ProviderCredentialInput::clear(),
            Self::Reference { alias } => ProviderCredentialInput::reference(alias)?,
            Self::Replace { value } => ProviderCredentialInput::replace(value)?,
        })
    }
}

struct VerificationStep {
    name: String,
    directory: Option<DirectoryCredentials>,
}

/// Parsed and SDK-validated complete desired state. Raw inputs never reach the
/// gateway or diagnostics; secrets have no Debug or output implementation.
pub(crate) struct ProviderCandidate {
    request: SecurityCandidateRequest,
    verification: Vec<VerificationStep>,
}

impl ProviderCandidate {
    pub(crate) fn read(path: &str) -> Result<Self, AppError> {
        let mut bytes = Vec::new();
        if path == "-" {
            io::stdin()
                .lock()
                .take(MAX_INPUT + 1)
                .read_to_end(&mut bytes)?;
        } else {
            let file = File::open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(invalid_input());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(AppError::InvalidOption(
                        "Provider input must be owner-only; run chmod 600 on it".into(),
                    ));
                }
            }
            file.take(MAX_INPUT + 1).read_to_end(&mut bytes)?;
        }
        if bytes.len() as u64 > MAX_INPUT {
            return Err(invalid_input());
        }
        Self::parse(&bytes)
    }

    fn parse(bytes: &[u8]) -> Result<Self, AppError> {
        let CandidateFile::Authentication {
            expected_revision,
            providers,
            release_ownership,
        } = serde_json::from_slice(bytes).map_err(|_| invalid_input())?;
        let mut updates = Vec::new();
        let mut verification = Vec::new();
        for provider in providers {
            let (update, step) = provider.into_update()?;
            updates.push(update);
            if let Some(step) = step {
                verification.push(step);
            }
        }
        let mut request = SecurityCandidateRequest::authentication(expected_revision, updates);
        if release_ownership {
            request = request.release_ownership();
        }
        Ok(Self {
            request,
            verification,
        })
    }
}

impl ProviderFile {
    fn into_update(
        mut self,
    ) -> Result<(AuthenticationProviderUpdate, Option<VerificationStep>), AppError> {
        let configuration = self.configuration_input()?;
        let mut update = match configuration {
            Some(configuration) => {
                AuthenticationProviderUpdate::runtime(&self.name, configuration)?
            }
            None => AuthenticationProviderUpdate::deployment_ldap(&self.name)?,
        }
        .enabled(self.enabled);
        if let Some(id) = self.provider_id {
            update = update.existing(id)?;
        }
        if self.adopt_deployment {
            update = update.adopt_deployment()?;
        }
        let step = if self.enabled {
            if self.kind == AuthenticationProviderKind::Ldap && self.verification.is_none() {
                return Err(AppError::InvalidOption("Enabled LDAP providers require verification username/password in the protected input".into()));
            }
            Some(VerificationStep {
                name: self.name,
                directory: self.verification,
            })
        } else {
            None
        };
        Ok((update, step))
    }

    fn configuration_input(&mut self) -> Result<Option<ProviderConfigurationInput>, AppError> {
        if self.source == SecurityConfigurationSource::Deployment {
            if self.kind != AuthenticationProviderKind::Ldap
                || self.configuration.is_some()
                || self.adopt_deployment
            {
                return Err(invalid_input());
            }
            return Ok(None);
        }
        let mut definition = self.configuration.take().ok_or_else(invalid_input)?;
        let result = match self.kind {
            AuthenticationProviderKind::Ldap => {
                let password = definition.remove("bind_password");
                let clear = password == Some(Value::Null);
                let password = match password {
                    Some(Value::String(value)) => Some(value),
                    None | Some(Value::Null) => None,
                    _ => return Err(invalid_input()),
                };
                let config = ProviderConfigurationInput::ldap(Value::Object(definition), password)?;
                if clear {
                    config.clear_ldap_bind_password()?
                } else {
                    config
                }
            }
            AuthenticationProviderKind::Oidc | AuthenticationProviderKind::Oauth => {
                if self.verification.is_some() {
                    return Err(invalid_input());
                }
                let credential: CredentialFile = serde_json::from_value(
                    definition.remove("credential").ok_or_else(invalid_input)?,
                )
                .map_err(|_| invalid_input())?;
                let credential = credential.into_input()?;
                if self.kind == AuthenticationProviderKind::Oidc {
                    ProviderConfigurationInput::oidc(Value::Object(definition), credential)?
                } else {
                    ProviderConfigurationInput::oauth(Value::Object(definition), credential)?
                }
            }
            _ => return Err(invalid_input()),
        };
        Ok(Some(result))
    }
}

fn candidate_view(report: &SecurityCandidateReport) -> Value {
    let state = match report.state {
        SecurityCandidateState::Unverified => "unverified",
        SecurityCandidateState::Verified => "verified",
        SecurityCandidateState::Failed => "failed",
        SecurityCandidateState::Expired => "expired",
        SecurityCandidateState::Activated => "activated",
        _ => "unsupported",
    };
    json!({"id":report.id,"group":report.group,"base_revision":report.base_revision,"expires_at":report.expires_at,
        "state":state,"providers":report.providers.iter().map(|p|json!({"provider_id":p.provider_id,"name":p.name,"kind":p.kind})).collect::<Vec<_>>(),
        "checks":report.checks.iter().map(|c|json!({"kind":c.kind,"status":c.status,"provider_id":c.provider_id,"reason":c.reason})).collect::<Vec<_>>()})
}

impl HubuumGateway {
    pub(crate) fn authentication_settings(&self) -> Result<Value, AppError> {
        let client = self.client();
        let state = client_call("client.effective_authentication_settings", || {
            client.effective_authentication_settings()
        })?;
        let providers = state.providers().iter().map(|provider| {
            let definition = provider.runtime_definition.as_ref().map(|definition| definition.iter().filter(|(key,_)| matches!(key.as_str(),
                "scope"|"url"|"bind_dn"|"user_base_dn"|"user_filter"|"subject_attribute"|"group_attributes"|"group_rules"|"issuer"|"client_id"|"client_authentication"|"scopes"|"signing_algorithms"|"retain_refresh_tokens"|"membership"|"admission"|"profile"|"authorization_endpoint"|"token_endpoint"|"profile_endpoint"|"subject_pointer"|"subject_type"|"name_pointer"
            )).map(|(key,value)|(key.clone(),value.clone())).collect::<Map<_,_>>());
            json!({"provider_id":provider.provider_id,"name":provider.name,"kind":provider.kind,"source":provider.source,"enabled":provider.enabled,
                "definition_mutable":provider.definition_mutable,"credential_configured":provider.credential_configured,"available":provider.available,"definition":definition,
                "callback_path":provider.provider_id.as_ref().filter(|_|matches!(provider.kind,AuthenticationProviderKind::Oidc|AuthenticationProviderKind::Oauth)).map(|id|format!("/api/v1/auth/callbacks/{}",id.as_str()))})
        }).collect::<Vec<_>>();
        let group = state.group();
        Ok(
            json!({"group":"authentication","source":group.source,"expected_revision":state.expected_revision(),"runtime_revision":group.revision,
            "mutable":group.mutable,"takes_effect":group.takes_effect,"replica":group.replica,"providers":providers,
            "registration":{"public_origin_source":"deployment","exact_callback_required":true},"local_authentication":"always_enabled"}),
        )
    }

    pub(crate) fn configure_authentication(
        &self,
        candidate: ProviderCandidate,
        activate: bool,
        no_browser: bool,
    ) -> Result<Value, AppError> {
        let client = self.client();
        let mut report = client_call("client.create_security_candidate", || {
            client.create_security_candidate(candidate.request)
        })?;
        eprintln!("Authentication candidate: {}", report.id.as_str());
        for provider in &report.providers {
            eprintln!(
                "Provider {}: {}",
                provider.name,
                provider.provider_id.as_str()
            );
            if matches!(
                provider.kind,
                AuthenticationProviderKind::Oidc | AuthenticationProviderKind::Oauth
            ) {
                eprintln!(
                    "Register callback: {}api/v1/auth/callbacks/{}",
                    client.base_url().as_str(),
                    provider.provider_id.as_str()
                );
            }
        }
        let mut previews = Vec::new();
        for step in candidate.verification {
            let provider = report
                .providers
                .iter()
                .find(|provider| provider.name == step.name)
                .ok_or(ApiError::InvalidAuthenticationInteraction)?
                .provider_id
                .clone();
            let verified = if let Some(directory) = step.directory {
                let request = LdapCandidateVerificationRequest::new(
                    provider,
                    directory.username,
                    directory.password,
                )?;
                client_call("client.verify_ldap_security_candidate", || {
                    client.verify_ldap_security_candidate(&report.id, request)
                })?
            } else {
                let driver = InteractionDriver::new()?;
                let request = AuthenticationInteractionRequest::new(
                    SessionClientId::new("hubuum-cli")?,
                    provider,
                );
                let mut flow =
                    client_call("client.begin_provider_verification_interaction", || {
                        client.begin_provider_verification_interaction(request, report.id.clone())
                    })?;
                if let Err(error) = driver.browser(&mut flow, no_browser) {
                    let _ = flow.cancel();
                    return Err(error);
                }
                if let Err(error) = driver.check_cancelled() {
                    let _ = flow.cancel();
                    return Err(error);
                }
                let verified = client_call("verification.complete", || flow.complete())?;
                driver.check_cancelled()?;
                verified
            };
            previews.push(json!({"provider_id":verified.verification.provider_id,"subject":verified.verification.subject,"admitted":verified.verification.admitted,
                "membership":verified.verification.membership,"groups":verified.verification.groups.iter().map(|group|json!({"key":group.key,"name":group.name})).collect::<Vec<_>>(),
                "issued_session":verified.verification.issued_session,"provisioned":verified.verification.provisioned}));
            report = verified.candidate;
        }
        let summary = candidate_view(&report);
        if activate {
            eprintln!(
                "Verified candidate {} at revision {}; activation requires fresh approval.",
                report.id.as_str(),
                report.base_revision
            );
            let operation =
                CredentialOperation::activate_security_candidate(report.into_activation()?);
            let approved = self.approval_source().approve(
                &client,
                operation,
                "activate the exact verified authentication provider candidate",
            )?;
            let result = send_approved(approved)?;
            Ok(
                json!({"candidate":summary,"previews":previews,"activation":{"group":result.group,"revision":result.revision,"changed":result.changed},"applied_everywhere":false}),
            )
        } else {
            Ok(json!({"candidate":summary,"previews":previews,"activated":false}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HubuumGateway, ProviderCandidate};
    use chrono::{Duration, Utc};
    use hubuum_client::{blocking::Client, MockTransport, Token, TransportResponse};
    use reqwest::StatusCode;
    use serde_json::{json, Value};
    use std::sync::Arc;

    fn input(kind: &str, enabled: bool) -> Value {
        let configuration = match kind {
            "ldap" => json!({"url":"ldaps://directory.example","bind_password":"private-password"}),
            "oidc" => {
                json!({"issuer":"https://issuer.example","credential":{"action":"replace","value":"private-client-secret"}})
            }
            _ => {
                json!({"authorization_endpoint":"https://issuer.example/auth","token_endpoint":"https://issuer.example/token","profile_endpoint":"https://issuer.example/me","credential":{"action":"replace","value":"private-client-secret"}})
            }
        };
        json!({"group":"authentication","expected_revision":7,"providers":[{"name":"employees","kind":kind,"enabled":enabled,"configuration":configuration}]})
    }

    #[test]
    fn each_external_kind_has_an_explicit_write_only_sdk_payload() {
        for kind in ["ldap", "oidc", "oauth"] {
            let plan = ProviderCandidate::parse(&serde_json::to_vec(&input(kind, false)).unwrap())
                .unwrap();
            let payload = serde_json::to_value(plan.request).unwrap();
            assert_eq!(payload["group"], "authentication");
            assert_eq!(payload["providers"][0]["kind"], kind);
            assert_eq!(payload["providers"][0]["enabled"], false);
            assert!(plan.verification.is_empty());
        }
    }

    #[test]
    fn ldap_omission_and_explicit_null_preserve_keep_clear_semantics() {
        for clear in [false, true] {
            let mut value = input("ldap", false);
            let config = value["providers"][0]["configuration"]
                .as_object_mut()
                .unwrap();
            config.remove("bind_password");
            if clear {
                config.insert("bind_password".into(), Value::Null);
            }
            let plan = ProviderCandidate::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            let payload = serde_json::to_value(plan.request).unwrap();
            assert_eq!(
                payload["providers"][0]["configuration"].get("bind_password"),
                clear.then_some(&Value::Null)
            );
        }
    }

    #[test]
    fn rejected_inputs_do_not_retain_secret_bearing_parser_details() {
        for value in [
            json!({"group":"private-unknown-group","expected_revision":7,"providers":[]}),
            json!({"group":"authentication","expected_revision":0,"providers":[]}),
            json!({"group":"authentication","expected_revision":7,"providers":[{"name":"local","kind":"ldap","enabled":false,"configuration":{"url":"ldaps://directory.example","bind_password":"private-password"}}]}),
            json!({"group":"authentication","expected_revision":7,"providers":[{"name":"employees","kind":"oidc","configuration":{"issuer":"http://user:private-password@issuer.example","credential":{"action":"private-unknown-action"}}}]}),
            input("ldap", true),
        ] {
            let error = ProviderCandidate::parse(&serde_json::to_vec(&value).unwrap())
                .err()
                .expect("must reject");
            assert!(!format!("{error:?}").contains("private-"));
        }
    }

    #[test]
    fn optimistic_conflicts_are_single_attempt_and_do_not_enter_approval() {
        let transport = MockTransport::default();
        transport.push_response(
            TransportResponse::json(
                StatusCode::PRECONDITION_FAILED,
                &json!({"error":"PreconditionFailed","message":"Stale revision"}),
            )
            .unwrap(),
        );
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("private-bearer"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let plan =
            ProviderCandidate::parse(&serde_json::to_vec(&input("oauth", false)).unwrap()).unwrap();
        assert!(gateway.configure_authentication(plan, true, true).is_err());
        assert_eq!(transport.requests().len(), 1);
    }

    #[test]
    fn preview_output_excludes_one_time_activation_receipt() {
        let transport = MockTransport::default();
        transport.push_response(TransportResponse::json(StatusCode::OK,&json!({
            "id":"aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa","group":"authentication","base_revision":7,"expires_at":Utc::now()+Duration::minutes(5),"checks":[],"state":"verified","verification_receipt":format!("hcv1.{}","a".repeat(64)),"providers":[]
        })).unwrap());
        let client = Client::builder_from_url("https://example.invalid")
            .unwrap()
            .with_transport(Arc::new(transport.clone()))
            .build()
            .unwrap()
            .authenticate(Token::new("private-bearer"));
        let gateway = HubuumGateway::new(Arc::new(client));
        let plan = ProviderCandidate::parse(
            br#"{"group":"authentication","expected_revision":7,"providers":[]}"#,
        )
        .unwrap();
        let output = gateway.configure_authentication(plan, false, true).unwrap();
        assert_eq!(output["activated"], false);
        assert!(!output.to_string().contains("hcv1."));
        assert_eq!(transport.requests().len(), 1);
    }
}
