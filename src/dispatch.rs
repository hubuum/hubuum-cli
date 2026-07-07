use std::sync::Arc;

use hubuum_client::ApiError;

use crate::app::{AppRuntime, SharedSession};
use crate::catalog::{
    CommandCatalog, CommandContext, CommandInvocation, CommandOutcome, ScopeAction,
};
use crate::errors::AppError;
use crate::output::{
    add_error, add_warning, append_line, reset_output, set_pipeline, set_render_format,
    take_output, OutputSnapshot,
};

pub async fn execute_line(
    app: Arc<AppRuntime>,
    session: &SharedSession,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    let (line, redirect) = prepare_redirect(&app.catalog, &session.scope(), line)?;
    let mut outcome = execute_line_inner(app, session, &line).await?;
    outcome.redirect = redirect;
    Ok(outcome)
}

async fn execute_line_inner(
    app: Arc<AppRuntime>,
    session: &SharedSession,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    reset_output()?;
    let (mut line, pipeline) = process_filter(line)?;
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
            ..Default::default()
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
            ..Default::default()
        });
    }

    if app.catalog.resolve_scope(&current_scope, &parts).is_some() {
        let mut next_scope = current_scope;
        next_scope.extend(parts);
        return Ok(CommandOutcome {
            output: Default::default(),
            scope_action: ScopeAction::Enter(next_scope),
            ..Default::default()
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
    set_render_format(crate::commands::render_format(&tokens)?)?;
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
        pipeline,
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

pub fn can_execute_offline(line: &str) -> bool {
    let line = match crate::redirection::split_redirect_candidate(line) {
        Ok(Some(candidate)) => candidate.line,
        Ok(None) => line.to_string(),
        Err(_) => return false,
    };
    let Ok(parts) = invocation_parts(&line) else {
        return false;
    };
    parts
        .first()
        .is_some_and(|part| part == "help" || part == "?")
        || command_path_is(&parts, &["config", "show"])
        || command_path_is(&parts, &["config", "paths"])
        || command_path_is(&parts, &["theme", "list"])
        || command_path_is(&parts, &["theme", "show"])
        || command_path_is(&parts, &["theme", "preview"])
}

pub fn execute_offline_line(
    catalog: &CommandCatalog,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    let (line, redirect) = prepare_redirect(catalog, &[], line)?;
    let mut outcome = execute_offline_line_inner(catalog, &line)?;
    outcome.redirect = redirect;
    Ok(outcome)
}

fn execute_offline_line_inner(
    catalog: &CommandCatalog,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    reset_output()?;
    let (line, _pipeline) = process_filter(line)?;
    let parts = invocation_parts(&line)?;
    if parts.is_empty() {
        return Ok(CommandOutcome::default());
    }

    if is_help_alias(&parts) {
        return render_help_from_catalog(catalog, Vec::new(), &parts[1..]);
    }

    if parts
        .first()
        .is_some_and(|part| part == "help" || part == "?")
    {
        if parts
            .iter()
            .skip(1)
            .any(|part| part == "--tree" || part == "-t")
        {
            append_line(catalog.render_tree())?;
            return Ok(CommandOutcome {
                output: take_output()?,
                scope_action: ScopeAction::None,
                ..Default::default()
            });
        }
        return render_help_from_catalog(catalog, Vec::new(), &parts[1..]);
    }

    if command_path_is(&parts, &["config", "show"]) {
        let resolved = catalog.resolve_command(&[], &parts)?;
        let tokens = tokenizer_for_resolved(&line, &resolved)?;
        set_render_format(crate::commands::render_format(&tokens)?)?;
        crate::commands::config::render_config_show(&tokens)?;
    } else if command_path_is(&parts, &["config", "paths"]) {
        let resolved = catalog.resolve_command(&[], &parts)?;
        let tokens = tokenizer_for_resolved(&line, &resolved)?;
        set_render_format(crate::commands::render_format(&tokens)?)?;
        crate::commands::config::render_config_paths(&tokens)?;
    } else if command_path_is(&parts, &["theme", "list"]) {
        let resolved = catalog.resolve_command(&[], &parts)?;
        let tokens = tokenizer_for_resolved(&line, &resolved)?;
        set_render_format(crate::commands::render_format(&tokens)?)?;
        crate::commands::theme::render_theme_list(&tokens)?;
    } else if command_path_is(&parts, &["theme", "show"]) {
        let resolved = catalog.resolve_command(&[], &parts)?;
        let tokens = tokenizer_for_resolved(&line, &resolved)?;
        set_render_format(crate::commands::render_format(&tokens)?)?;
        crate::commands::theme::render_theme_show(&tokens)?;
    } else if command_path_is(&parts, &["theme", "preview"]) {
        let resolved = catalog.resolve_command(&[], &parts)?;
        let tokens = tokenizer_for_resolved(&line, &resolved)?;
        set_render_format(crate::commands::render_format(&tokens)?)?;
        crate::commands::theme::render_theme_preview(&tokens)?;
    } else {
        catalog.resolve_command(&[], &parts)?;
        return Err(AppError::CommandNotFound(parts.join(" ")));
    }

    Ok(CommandOutcome {
        output: take_output()?,
        scope_action: ScopeAction::None,
        ..Default::default()
    })
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
    render_help_from_catalog(app.catalog.as_ref(), scope, parts)
}

fn render_help_from_catalog(
    catalog: &CommandCatalog,
    scope: Vec<String>,
    parts: &[String],
) -> Result<CommandOutcome, AppError> {
    if parts.is_empty() {
        append_line(catalog.render_scope_help(&scope))?;
    } else if parts.first().map(String::as_str) == Some("pipe") {
        append_line(catalog.render_pipe_topic_help(parts.get(1).map(String::as_str))?)?;
    } else if parts.first().map(String::as_str) == Some("shell") {
        append_line(catalog.render_shell_topic_help(parts.get(1).map(String::as_str))?)?;
    } else if let Ok(resolved) = catalog.resolve_command(&scope, parts) {
        append_line(catalog.render_command_help(&resolved.command_path)?)?;
    } else if let Some(_nested_scope) = catalog.resolve_scope(&scope, parts) {
        let mut nested_path = scope.clone();
        nested_path.extend_from_slice(parts);
        append_line(catalog.render_scope_help(&nested_path))?;
    } else {
        return Err(AppError::CommandNotFound(parts.join(" ")));
    }

    Ok(CommandOutcome {
        output: take_output()?,
        scope_action: ScopeAction::None,
        ..Default::default()
    })
}

fn prepare_redirect(
    catalog: &CommandCatalog,
    scope: &[String],
    line: &str,
) -> Result<(String, Option<crate::redirection::OutputRedirect>), AppError> {
    let Some(candidate) = crate::redirection::split_redirect_candidate(line)? else {
        return Ok((line.to_string(), None));
    };

    if parses_as_command(catalog, scope, &candidate.line) {
        Ok((candidate.line, Some(candidate.redirect)))
    } else {
        Ok((line.to_string(), None))
    }
}

fn parses_as_command(catalog: &CommandCatalog, scope: &[String], line: &str) -> bool {
    let Ok((line, _pipeline)) = process_filter(line) else {
        return false;
    };
    let Ok(parts) = invocation_parts(&line) else {
        return false;
    };

    if parts.is_empty() || is_help_alias(&parts) {
        return true;
    }
    if matches!(
        parts.first().map(String::as_str),
        Some("next" | "exit" | "quit" | "..")
    ) {
        return true;
    }
    if catalog.resolve_scope(scope, &parts).is_some() {
        return true;
    }

    let Ok(resolved) = catalog.resolve_command(scope, &parts) else {
        return false;
    };
    tokenizer_for_resolved(&line, &resolved).is_ok()
}

fn process_filter(line: &str) -> Result<(String, Vec<hubuum_filter::PipeStage>), AppError> {
    let (command, pipeline) = hubuum_filter::split_pipeline(line)?;
    set_pipeline(pipeline.clone())?;
    Ok((command, pipeline))
}

fn invocation_parts(line: &str) -> Result<Vec<String>, AppError> {
    shlex::split(line).ok_or_else(|| AppError::ParseError("Parsing input failed".to_string()))
}

fn command_path_is(parts: &[String], expected: &[&str]) -> bool {
    parts.len() >= expected.len()
        && parts
            .iter()
            .zip(expected.iter())
            .all(|(part, expected)| part == expected)
}

fn tokenizer_for_resolved(
    line: &str,
    resolved: &crate::catalog::ResolvedCommand<'_>,
) -> Result<crate::tokenizer::CommandTokenizer, AppError> {
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
    crate::tokenizer::CommandTokenizer::new(line, &cmd_name, &option_defs)
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::{
        apply_output_state, can_execute_offline, execute_offline_line, is_help_alias,
        prepare_redirect, process_filter,
    };
    use crate::app::SharedSession;
    use crate::output::{append_line, reset_output, take_output};
    use crate::redirection::RedirectTarget;

    #[test]
    #[serial]
    fn process_filter_sets_runtime_filter() {
        reset_output().expect("buffer should reset");
        let (line, _pipeline) = process_filter("list | alpha").expect("filter should parse");
        assert_eq!(line, "list");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture filtered output");
        assert_eq!(snapshot.lines, vec!["alpha".to_string()]);
    }

    #[test]
    #[serial]
    fn process_filter_applies_multiple_pipe_stages() {
        reset_output().expect("buffer should reset");
        let (line, _pipeline) = process_filter("list | reject beta | sort line desc | head 1")
            .expect("filter should parse");
        assert_eq!(line, "list");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");
        append_line("gamma").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture filtered output");
        assert_eq!(snapshot.lines, vec!["gamma".to_string()]);
    }

    #[test]
    #[serial]
    fn process_filter_ignores_quoted_pipes() {
        reset_output().expect("buffer should reset");
        let (line, _pipeline) =
            process_filter("object list --where name equals 'alpha|beta' | beta").expect("filter");
        assert_eq!(line, "object list --where name equals 'alpha|beta'");
        append_line("alpha|beta").expect("line should append");
        append_line("alpha").expect("line should append");

        let snapshot = take_output().expect("snapshot should capture filtered output");
        assert_eq!(snapshot.lines, vec!["alpha|beta".to_string()]);
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

    #[test]
    fn offline_command_detection_is_limited_to_catalog_and_config_inspection() {
        assert!(can_execute_offline("help --tree"));
        assert!(can_execute_offline("config show --key server.hostname"));
        assert!(can_execute_offline("config paths"));
        assert!(can_execute_offline("theme list"));
        assert!(can_execute_offline("theme preview hubuum-dark"));
        assert!(!can_execute_offline("theme use hubuum-dark"));
        assert!(!can_execute_offline(
            "config set --key server.hostname --value localhost"
        ));
        assert!(!can_execute_offline("object list --limit 5"));
    }

    #[test]
    fn offline_unknown_commands_include_catalog_suggestion() {
        let catalog = crate::commands::build_command_catalog();
        let err = execute_offline_line(&catalog, "clas list")
            .expect_err("mistyped offline command should fail");

        assert!(err.to_string().contains("Did you mean 'class'?"));
    }

    #[test]
    fn offline_redirect_is_attached_and_removed_from_command() {
        let catalog = crate::commands::build_command_catalog();
        let outcome = execute_offline_line(&catalog, "help > help.txt")
            .expect("offline redirect should execute");

        assert_eq!(
            outcome.redirect.as_ref().map(|redirect| &redirect.target),
            Some(&RedirectTarget::File(std::path::PathBuf::from("help.txt")))
        );
        assert!(outcome
            .output
            .lines
            .iter()
            .any(|line| line.contains("Commands:")));
    }

    #[test]
    fn redirect_detection_does_not_consume_filter_operator() {
        let catalog = crate::commands::build_command_catalog();
        let (line, redirect) = prepare_redirect(&catalog, &[], "object list --where age > 3")
            .expect("redirect preparation should succeed");

        assert_eq!(line, "object list --where age > 3");
        assert!(redirect.is_none());
    }

    #[test]
    fn redirect_detection_accepts_redirect_after_filter_operator() {
        let catalog = crate::commands::build_command_catalog();
        let (line, redirect) =
            prepare_redirect(&catalog, &[], "object list --where age > 3 > out.json")
                .expect("redirect preparation should succeed");

        assert_eq!(line, "object list --where age > 3");
        assert_eq!(
            redirect.as_ref().map(|redirect| &redirect.target),
            Some(&RedirectTarget::File(std::path::PathBuf::from("out.json")))
        );
    }
}
