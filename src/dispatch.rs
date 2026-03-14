use std::sync::Arc;

use hubuum_client::ApiError;

use crate::app::{AppRuntime, SharedSession};
use crate::catalog::{CommandContext, CommandInvocation, CommandOutcome, ScopeAction};
use crate::errors::AppError;
use crate::output::{
    add_error, add_warning, append_line, clear_filter, reset_output, set_filter, take_output,
    OutputSnapshot,
};

pub async fn execute_line(
    app: Arc<AppRuntime>,
    session: &SharedSession,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    reset_output()?;
    let mut line = process_filter(line)?;
    let mut parts = shlex::split(&line)
        .ok_or_else(|| AppError::ParseError("Parsing input failed".to_string()))?;

    if parts.len() == 1 && parts[0] == "next" {
        let Some(next_page_command) = session.next_page_command() else {
            return Ok(CommandOutcome::default());
        };
        line = next_page_command;
        parts = shlex::split(&line)
            .ok_or_else(|| AppError::ParseError("Parsing input failed".to_string()))?;
    }

    if parts.is_empty() {
        return Ok(CommandOutcome::default());
    }

    if is_help_alias(&parts) {
        return render_help(app, session.scope(), &parts[1..]);
    }

    if parts[0] == "exit" || parts[0] == "quit" {
        return Ok(CommandOutcome {
            output: Default::default(),
            scope_action: if session.scope().is_empty() {
                ScopeAction::ExitRepl
            } else {
                ScopeAction::ExitScope
            },
        });
    }

    let current_scope = session.scope();
    if parts.len() == 1 && parts[0] == ".." {
        return Ok(CommandOutcome {
            output: Default::default(),
            scope_action: if current_scope.is_empty() {
                ScopeAction::ExitRepl
            } else {
                ScopeAction::ExitScope
            },
        });
    }

    if app.catalog.resolve_scope(&current_scope, &parts).is_some() {
        let mut next_scope = current_scope;
        next_scope.extend(parts);
        return Ok(CommandOutcome {
            output: Default::default(),
            scope_action: ScopeAction::Enter(next_scope),
        });
    }

    let resolved = app.catalog.resolve_command(&current_scope, &parts)?;
    let cmd_name = resolved
        .command_path
        .last()
        .cloned()
        .ok_or_else(|| AppError::CommandExecutionError("Missing command name".to_string()))?;
    let option_defs = resolved
        .command
        .options
        .iter()
        .map(|option| option.to_cli_option())
        .collect::<Vec<_>>();
    let tokens = crate::tokenizer::CommandTokenizer::new(&line, &cmd_name, &option_defs)?;
    let options = tokens.get_options();
    if options.contains_key("help") || options.contains_key("h") {
        return render_help(
            app.clone(),
            resolved.scope_path.clone(),
            &resolved.command_path[resolved.scope_path.len()..],
        );
    }
    let invocation = CommandInvocation {
        raw_line: line.clone(),
        command_path: resolved.command_path.clone(),
    };
    let ctx = CommandContext { app: app.clone() };

    resolved.command.handler.execute(ctx, invocation).await
}

fn is_help_alias(parts: &[String]) -> bool {
    matches!(parts.first().map(String::as_str), Some("help" | "?"))
        && !parts.iter().skip(1).any(|part| part.starts_with('-'))
}

pub async fn execute_script(
    app: Arc<AppRuntime>,
    session: &SharedSession,
    filename: &str,
) -> Result<Vec<CommandOutcome>, AppError> {
    let content = tokio::fs::read_to_string(filename).await?;
    let mut outcomes = Vec::new();
    for line in content.lines() {
        outcomes.push(execute_line(app.clone(), session, line).await?);
    }
    Ok(outcomes)
}

pub fn apply_scope_action(session: &SharedSession, action: &ScopeAction) {
    match action {
        ScopeAction::None => {}
        ScopeAction::Enter(scope) => session.set_scope(scope.clone()),
        ScopeAction::ExitScope => {
            session.exit_scope();
        }
        ScopeAction::ExitRepl => {}
    }
}

pub fn apply_output_state(session: &SharedSession, output: &OutputSnapshot) {
    session.set_next_page_command(output.next_page_command.clone());
}

pub fn render_error(err: AppError) -> crate::output::OutputSnapshot {
    reset_output().expect("reset output buffer for errors");
    match err {
        AppError::Quiet => {}
        AppError::EntityNotFound(entity) => {
            add_warning(entity).expect("warning should be added");
        }
        AppError::ApiError(ApiError::HttpWithBody { status, message }) => {
            add_error(format!("API Error: Status {status} - {message}"))
                .expect("error should be added");
        }
        AppError::ApiError(api_error) => {
            add_error(format!("API Error: {api_error}")).expect("error should be added");
        }
        other => {
            add_error(other).expect("error should be added");
        }
    }
    take_output().expect("error snapshot should be captured")
}

fn render_help(
    app: Arc<AppRuntime>,
    scope: Vec<String>,
    parts: &[String],
) -> Result<CommandOutcome, AppError> {
    reset_output()?;

    if parts.is_empty() {
        append_line(app.catalog.render_scope_help(&scope))?;
    } else if let Ok(resolved) = app.catalog.resolve_command(&scope, parts) {
        append_line(app.catalog.render_command_help(&resolved.command_path)?)?;
    } else if let Some(_nested_scope) = app.catalog.resolve_scope(&scope, parts) {
        let mut nested_path = scope.clone();
        nested_path.extend_from_slice(parts);
        append_line(app.catalog.render_scope_help(&nested_path))?;
    } else {
        return Err(AppError::CommandNotFound(parts.join(" ")));
    }

    Ok(CommandOutcome {
        output: take_output()?,
        scope_action: ScopeAction::None,
    })
}

fn process_filter(line: &str) -> Result<String, AppError> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() > 1 {
        let filter = parts[1].trim();
        let (invert, pattern) = if let Some(stripped) = filter.strip_prefix('!') {
            (true, stripped.trim())
        } else {
            (false, filter.trim())
        };
        set_filter(pattern.to_string(), invert)?;
        Ok(parts[0].trim().to_string())
    } else {
        clear_filter()?;
        Ok(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    use httpmock::prelude::*;
    use hubuum_client::{ApiError, Credentials, SyncClient};
    use rstest::rstest;
    use serial_test::serial;
    use tempfile::NamedTempFile;
    use tokio::runtime::{Handle, Runtime};

    use super::{
        apply_output_state, execute_line, execute_script, is_help_alias, process_filter,
        render_error,
    };
    use crate::app::{AppRuntime, SharedSession};
    use crate::catalog::ScopeAction;
    use crate::commands::build_command_catalog;
    use crate::config::{init_config, AppConfig};
    use crate::errors::AppError;
    use crate::output::{append_line, reset_output, take_output};
    use crate::services::AppServices;

    fn mock_login(server: &MockServer) {
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v0/auth/login")
                .json_body_obj(&serde_json::json!({
                    "username": "tester",
                    "password": "secret",
                }));
            then.status(200)
                .header("content-type", "application/json")
                .json_body_obj(&serde_json::json!({ "token": "test-token" }));
        });
    }

    fn runtime_for_tests(server: &MockServer, handle: Handle) -> Arc<AppRuntime> {
        let base_url =
            hubuum_client::BaseUrl::from_str(&server.base_url()).expect("base URL should be valid");
        let client = SyncClient::new_with_certificate_validation(base_url, true)
            .login(Credentials::new("tester".to_string(), "secret".to_string()))
            .expect("login should succeed");

        init_config(AppConfig::default()).expect("config should initialize");

        Arc::new(AppRuntime::new(
            Arc::new(AppConfig::default()),
            Arc::new(AppServices::new(
                Arc::new(client),
                handle,
                Duration::from_secs(1),
            )),
            Arc::new(build_command_catalog()),
        ))
    }

    #[test]
    #[serial]
    fn process_filter_sets_runtime_filter() {
        reset_output().expect("buffer should reset");
        let line = process_filter("list | alpha").expect("filter should parse");
        assert_eq!(line, "list");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture filtered output");
        assert_eq!(snapshot.lines, vec!["alpha".to_string()]);
    }

    #[test]
    #[serial]
    fn process_filter_supports_inversion() {
        reset_output().expect("buffer should reset");
        let line = process_filter("list | !alpha").expect("filter should parse");
        assert_eq!(line, "list");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture filtered output");
        assert_eq!(snapshot.lines, vec!["beta".to_string()]);
    }

    #[test]
    #[serial]
    fn process_filter_clears_previous_filter_on_plain_line() {
        reset_output().expect("buffer should reset");
        process_filter("list | alpha").expect("filter should parse");
        process_filter("list").expect("plain line should clear filter");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture output");
        assert_eq!(
            snapshot.lines,
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn process_filter_rejects_invalid_regex() {
        let err = process_filter("list | [").expect_err("invalid regex should fail");
        assert!(matches!(err, AppError::RegexError(_)));
    }

    #[test]
    #[serial]
    fn help_alias_accepts_question_mark() {
        assert!(is_help_alias(&["?".to_string(), "class".to_string()]));
        assert!(is_help_alias(&["help".to_string()]));
        assert!(!is_help_alias(&["?".to_string(), "--tree".to_string()]));
    }

    #[test]
    fn apply_output_state_tracks_next_page_command() {
        let session = SharedSession::new();
        apply_output_state(
            &session,
            &crate::output::OutputSnapshot {
                next_page_command: Some("object list --cursor abc".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(
            session.next_page_command().as_deref(),
            Some("object list --cursor abc")
        );

        apply_output_state(&session, &crate::output::OutputSnapshot::default());
        assert!(session.next_page_command().is_none());
    }

    #[rstest]
    #[case(AppError::Quiet, None, None)]
    #[case(
        AppError::EntityNotFound("missing".to_string()),
        Some("missing"),
        None
    )]
    #[case(
        AppError::ApiError(ApiError::HttpWithBody {
            status: reqwest::StatusCode::BAD_REQUEST,
            message: "bad input".to_string(),
        }),
        None,
        Some("API Error: Status 400 Bad Request - bad input")
    )]
    #[case(
        AppError::ParseError("broken".to_string()),
        None,
        Some("Error parsing arguments: broken")
    )]
    fn render_error_maps_variants_to_snapshot(
        #[case] err: AppError,
        #[case] warning: Option<&str>,
        #[case] error: Option<&str>,
    ) {
        let snapshot = render_error(err);
        assert_eq!(snapshot.warnings.first().map(String::as_str), warning);
        assert_eq!(snapshot.errors.first().map(String::as_str), error);
    }

    #[test]
    fn execute_line_enters_scope_when_given_scope_name() {
        let test_runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server);
        let runtime = runtime_for_tests(&server, test_runtime.handle().clone());
        test_runtime.block_on(async {
            let session = SharedSession::new();

            let outcome = execute_line(runtime.clone(), &session, "object")
                .await
                .expect("scope entry should succeed");

            assert_eq!(
                outcome.scope_action,
                ScopeAction::Enter(vec!["object".to_string()])
            );
            assert!(outcome.output.is_empty());
        });
    }

    #[test]
    fn execute_line_renders_help_from_next_page_command() {
        let test_runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server);
        let runtime = runtime_for_tests(&server, test_runtime.handle().clone());
        test_runtime.block_on(async {
            let session = SharedSession::new();
            session.set_next_page_command(Some("help object".to_string()));

            let outcome = execute_line(runtime.clone(), &session, "next")
                .await
                .expect("next command should succeed");

            assert!(outcome
                .output
                .lines
                .iter()
                .any(|line| line.contains("Scope: object")));
        });
    }

    #[test]
    fn execute_line_returns_default_when_next_page_command_is_missing() {
        let test_runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server);
        let runtime = runtime_for_tests(&server, test_runtime.handle().clone());
        test_runtime.block_on(async {
            let session = SharedSession::new();

            let outcome = execute_line(runtime.clone(), &session, "next")
                .await
                .expect("missing next page command should not fail");

            assert!(outcome.output.is_empty());
            assert_eq!(outcome.scope_action, ScopeAction::None);
        });
    }

    #[test]
    fn execute_script_runs_help_lines_in_order() {
        let test_runtime = Runtime::new().expect("runtime should build");
        let server = MockServer::start();
        mock_login(&server);
        let script = NamedTempFile::new().expect("temp script should be created");
        std::fs::write(script.path(), "help\nhelp object\n").expect("script should be written");
        let runtime = runtime_for_tests(&server, test_runtime.handle().clone());

        test_runtime.block_on(async {
            let session = SharedSession::new();

            let outcomes = execute_script(
                runtime.clone(),
                &session,
                script.path().to_str().expect("script path should be utf-8"),
            )
            .await
            .expect("script should execute");

            assert_eq!(outcomes.len(), 2);
            assert!(outcomes[0]
                .output
                .lines
                .iter()
                .any(|line| line.contains("Available commands")));
            assert!(outcomes[1]
                .output
                .lines
                .iter()
                .any(|line| line.contains("Scope: object")));
        });
    }
}
