use std::env::args;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use app::{init_logging, load_app_config, login, AppRuntime, SharedSession};
use catalog::{CommandCatalog, CommandOutcome};
use cli::{build_cli, execution_mode, split_startup_args, StartupMode};
use commands::build_command_catalog;
use dispatch::{
    apply_output_state, apply_scope_action, can_execute_offline, execute_line,
    execute_offline_line, render_error,
};
use errors::AppError;
use output::{print_rendered, OutputSnapshot};
use redirection::write_output;
use repl::run;
use services::AppServices;
use tokio::fs::read_to_string;
use tokio::runtime::Handle;

mod app;
mod autocomplete;
mod background;
mod catalog;
mod cli;
mod command_line;
mod commands;
mod config;
mod defaults;
mod dispatch;
mod domain;
mod errors;
mod files;
mod formatting;
mod json_schema;
mod list_query;
mod models;
mod output;
mod redirection;
mod repl;
mod services;
mod suggestions;
mod terminal;
mod theme;
mod tokenizer;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), AppError> {
    let startup_args = split_startup_args(args());
    let matches = build_cli().get_matches_from(startup_args.clap_args);
    let config = load_app_config(&matches)?;
    let catalog = Arc::new(build_command_catalog());
    let mode = execution_mode(&matches, startup_args.mode);

    match &mode {
        StartupMode::Command(command) if can_execute_offline(command) => {
            let outcome = execute_offline_line(catalog.as_ref(), command);
            if !render_dispatch_result(&sessionless(), outcome) {
                exit(1);
            }
            return Ok(());
        }
        StartupMode::Script(filename) if can_execute_script_offline(filename).await? => {
            let session = SharedSession::new();
            if !execute_offline_script(catalog.as_ref(), &session, filename).await? {
                exit(1);
            }
            return Ok(());
        }
        StartupMode::Repl | StartupMode::Command(_) | StartupMode::Script(_) => {}
    }

    init_logging()?;
    let client = login(config.clone()).await?;

    let services = Arc::new(AppServices::new(
        client,
        Handle::current(),
        Duration::from_secs(config.background.poll_interval_seconds),
    ));
    let runtime = Arc::new(AppRuntime::new(config, services, catalog));
    let session = SharedSession::new();

    if let StartupMode::Command(command) = mode {
        let outcome = execute_line(runtime.clone(), &session, &command).await;
        if !render_dispatch_result(&session, outcome) {
            exit(1);
        }
        return Ok(());
    }

    if let StartupMode::Script(filename) = mode {
        if !execute_script(runtime.clone(), &session, &filename).await? {
            exit(1);
        }
        return Ok(());
    }

    run(runtime, session).await
}

fn sessionless() -> SharedSession {
    SharedSession::new()
}

fn render_dispatch_result(
    session: &SharedSession,
    result: Result<CommandOutcome, AppError>,
) -> bool {
    match result {
        Ok(outcome) => render_outcome(session, outcome),
        Err(err) => {
            render_snapshot(render_error(err));
            false
        }
    }
}

async fn execute_script(
    runtime: Arc<AppRuntime>,
    session: &SharedSession,
    filename: &str,
) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    for line in content.lines() {
        let outcome = execute_line(runtime.clone(), session, line).await;
        if !render_dispatch_result(session, outcome) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn can_execute_script_offline(filename: &str) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(can_execute_offline))
}

async fn execute_offline_script(
    catalog: &CommandCatalog,
    session: &SharedSession,
    filename: &str,
) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    for line in content.lines() {
        let outcome = execute_offline_line(catalog, line);
        if !render_dispatch_result(session, outcome) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn render_outcome(session: &SharedSession, outcome: CommandOutcome) -> bool {
    apply_scope_action(session, &outcome.scope_action);
    apply_output_state(session, &outcome.output);
    match outcome.redirect {
        Some(redirect) => match write_output(&outcome.output, &redirect) {
            Ok(()) => true,
            Err(err) => {
                render_snapshot(render_error(err));
                false
            }
        },
        None => {
            render_snapshot(outcome.output);
            true
        }
    }
}

fn render_snapshot(snapshot: OutputSnapshot) {
    if !snapshot.is_empty() {
        let _ = print_rendered(&snapshot.render());
    }
}
