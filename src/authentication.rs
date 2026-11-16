//! Application hooks around SDK-owned interactions; no provider protocol lives here.
use std::io::{self, IsTerminal};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use hubuum_client::client::sync::AuthenticationInteraction;
use hubuum_client::{ApiError, AuthenticationInteractionState, AuthenticationNext};

use crate::errors::AppError;

const ACTIVE: usize = 1;
const CANCELLED: usize = 2;
static STATE: AtomicUsize = AtomicUsize::new(0);

/// Tokio retains the OS signal handler after its first registration. Preserve
/// ordinary process interruption outside an interaction, including a second
/// interrupt while bounded cancellation/revocation is in progress.
pub(crate) fn install_interrupt_handler() {
    tokio::spawn(async {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            if STATE
                .compare_exchange(ACTIVE, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                std::process::exit(130);
            }
        }
    });
}

/// Keep this guard alive through completion/persistence and any exact cleanup.
pub(crate) struct InteractionDriver {
    state: &'static AtomicUsize,
}

impl InteractionDriver {
    pub(crate) fn new() -> Result<Self, AppError> {
        STATE
            .compare_exchange(0, ACTIVE, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                AppError::CommandExecutionError(
                    "Another authentication interaction is active".into(),
                )
            })?;
        Ok(Self { state: &STATE })
    }

    pub(crate) fn check_cancelled(&self) -> Result<(), AppError> {
        if self.state.load(Ordering::SeqCst) == CANCELLED {
            Err(AppError::CommandExecutionError(
                "Authentication cancelled; no operation was replayed".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn wait(&self, delay: Duration) -> Result<(), AppError> {
        let until = Instant::now() + delay;
        loop {
            self.check_cancelled()?;
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            thread::sleep(left.min(Duration::from_millis(100)));
        }
    }

    pub(crate) fn browser<C>(
        &self,
        flow: &mut AuthenticationInteraction<C>,
        no_browser: bool,
    ) -> Result<(), AppError> {
        let AuthenticationNext::Browser {
            verification_uri,
            user_code,
        } = flow.next()
        else {
            return Err(AppError::CommandExecutionError(
                "This operation requires an explicit browser authentication step".into(),
            ));
        };
        if !no_browser && !io::stdin().is_terminal() {
            return Err(AppError::CommandExecutionError(
                "interactive_login_required: use --no-browser explicitly to display remote-browser instructions".into(),
            ));
        }
        // These are public, SDK-validated instructions. Redemption proofs never
        // enter output, browser URLs, child arguments or process environments.
        eprintln!("Open {verification_uri}\nVerification code: {user_code}");
        if !no_browser && launch_browser(verification_uri).is_err() {
            eprintln!("Could not launch a browser; use the verification instructions above.");
        }
        loop {
            self.wait(flow.poll_after())?;
            match flow.poll() {
                Ok(status) if status.state == AuthenticationInteractionState::Ready => {
                    return self.check_cancelled();
                }
                Ok(status) if status.state == AuthenticationInteractionState::Pending => {}
                Ok(status) => return Err(terminal_error(status.state)),
                Err(error) if retryable_status(&error) => {}
                Err(error) => return Err(interaction_error(error)),
            }
        }
    }
}

impl Drop for InteractionDriver {
    fn drop(&mut self) {
        self.state.store(0, Ordering::SeqCst);
    }
}

pub(crate) fn interaction_error(error: ApiError) -> AppError {
    if error.is_capability_unsupported() {
        AppError::CommandExecutionError("capability_unsupported: This provider cannot freshly approve the operation. The protected change was not executed; no password fallback was attempted.".into())
    } else if matches!(error, ApiError::AuthenticationInteractionExpired) {
        AppError::CommandExecutionError("authentication_expired: Begin a new interaction".into())
    } else {
        error.into()
    }
}

fn terminal_error(state: AuthenticationInteractionState) -> AppError {
    let message = match state {
        AuthenticationInteractionState::Denied => "authentication_denied: The sign-in was denied",
        AuthenticationInteractionState::Expired => "authentication_expired: Begin a new interaction",
        AuthenticationInteractionState::Cancelled => "authentication_cancelled: Begin a new interaction",
        AuthenticationInteractionState::Consumed => "authentication_consumed: Completion was already used; inspect the session before signing in again",
        AuthenticationInteractionState::Failed => "authentication_failed: The server stopped authentication; begin a new interaction",
        _ => return ApiError::InvalidAuthenticationInteraction.into(),
    };
    AppError::CommandExecutionError(message.into())
}

fn retryable_status(error: &ApiError) -> bool {
    (matches!(error, ApiError::Http(_)) && error.status().is_none())
        || error.status().is_some_and(|status| {
            status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        })
}

fn launch_browser(uri: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    for (name, _) in std::env::vars_os() {
        if name
            .to_str()
            .is_some_and(|name| name.to_ascii_uppercase().starts_with("HUBUUM_"))
        {
            command.env_remove(name);
        }
    }
    let mut child = command
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::InteractionDriver;
    use chrono::{Duration as ChronoDuration, Utc};
    use hubuum_client::{
        blocking::Client, AuthenticationInteractionRequest, AuthenticationProviderId,
        MockTransport, SessionClientId, TransportResponse,
    };
    use reqwest::StatusCode;
    use serde_json::json;
    use serial_test::serial;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn terminal_outcomes_stop_polling_without_completion_or_password_fallback() {
        for (state, reason) in [
            ("denied", "authentication_denied"),
            ("expired", "authentication_expired"),
            ("cancelled", "authentication_cancelled"),
            ("consumed", "authentication_consumed"),
            ("failed", "authentication_failed"),
        ] {
            let transport = MockTransport::default();
            let expiry = Utc::now() + ChronoDuration::minutes(5);
            transport.push_response(TransportResponse::json(StatusCode::OK,&json!({
                "interaction_version":1,"id":"aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa","redemption_secret":format!("hai1.{}","a".repeat(64)),"expires_at":expiry,"interval_seconds":1,"next":{"kind":"browser","verification_uri":"https://example.invalid/verify","user_code":"PUBLIC-CODE"}
            })).unwrap());
            transport.push_response(TransportResponse::json(StatusCode::OK,&json!({
                "id":"aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa","state":state,"expires_at":expiry
            })).unwrap());
            let client = Client::builder_from_url("https://example.invalid")
                .unwrap()
                .with_transport(Arc::new(transport.clone()))
                .build()
                .unwrap();
            let mut flow = client
                .begin_login_interaction(AuthenticationInteractionRequest::new(
                    SessionClientId::new("hubuum-cli").unwrap(),
                    AuthenticationProviderId::new("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb").unwrap(),
                ))
                .unwrap();
            let driver = InteractionDriver::new().unwrap();
            let error = driver.browser(&mut flow, true).unwrap_err();
            assert!(error.to_string().contains(reason), "{state}: {error}");
            assert_eq!(transport.requests().len(), 2);
        }
    }
}
