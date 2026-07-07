use std::sync::Arc;
use std::time::Duration;

use app::{AppRuntime, SharedSession};
use errors::AppError;
use services::AppServices;

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
mod repl;
mod services;
mod suggestions;
mod terminal;
mod theme;
mod tokenizer;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), AppError> {
    let startup_args = cli::split_startup_args(std::env::args());
    let matches = cli::build_cli().get_matches_from(startup_args.clap_args);
    app::init_logging()?;
    let config = app::load_app_config(&matches)?;
    let catalog = Arc::new(commands::build_command_catalog());
    let mode = cli::execution_mode(&matches, startup_args.mode);

    match &mode {
        cli::StartupMode::Command(command) if dispatch::can_execute_offline(command) => {
            let outcome = dispatch::execute_offline_line(catalog.as_ref(), command);
            if !render_dispatch_result(&sessionless(), outcome) {
                std::process::exit(1);
            }
            return Ok(());
        }
        cli::StartupMode::Script(filename) if can_execute_script_offline(filename).await? => {
            let session = SharedSession::new();
            let outcomes = execute_offline_script(catalog.as_ref(), filename).await;
            if !render_script_result(&session, outcomes) {
                std::process::exit(1);
            }
            return Ok(());
        }
        cli::StartupMode::Repl | cli::StartupMode::Command(_) | cli::StartupMode::Script(_) => {}
    }

    let client = app::login(config.clone()).await?;

    let services = Arc::new(AppServices::new(
        client,
        tokio::runtime::Handle::current(),
        Duration::from_secs(config.background.poll_interval_seconds),
    ));
    let runtime = Arc::new(AppRuntime::new(config, services, catalog));
    let session = SharedSession::new();

    if let cli::StartupMode::Command(command) = mode {
        let outcome = dispatch::execute_line(runtime.clone(), &session, &command).await;
        if !render_dispatch_result(&session, outcome) {
            std::process::exit(1);
        }
        return Ok(());
    }

    if let cli::StartupMode::Script(filename) = mode {
        let outcomes = dispatch::execute_script(runtime.clone(), &session, &filename).await;
        if !render_script_result(&session, outcomes) {
            std::process::exit(1);
        }
        return Ok(());
    }

    repl::run(runtime, session).await
}

fn sessionless() -> SharedSession {
    SharedSession::new()
}

fn render_dispatch_result(
    session: &SharedSession,
    result: Result<catalog::CommandOutcome, AppError>,
) -> bool {
    match result {
        Ok(outcome) => {
            render_outcome(session, outcome);
            true
        }
        Err(err) => {
            render_snapshot(dispatch::render_error(err));
            false
        }
    }
}

fn render_script_result(
    session: &SharedSession,
    outcomes: Result<Vec<catalog::CommandOutcome>, AppError>,
) -> bool {
    match outcomes {
        Ok(outcomes) => {
            for outcome in outcomes {
                render_outcome(session, outcome);
            }
            true
        }
        Err(err) => {
            render_snapshot(dispatch::render_error(err));
            false
        }
    }
}

async fn can_execute_script_offline(filename: &str) -> Result<bool, AppError> {
    let content = tokio::fs::read_to_string(filename).await?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(dispatch::can_execute_offline))
}

async fn execute_offline_script(
    catalog: &catalog::CommandCatalog,
    filename: &str,
) -> Result<Vec<catalog::CommandOutcome>, AppError> {
    let content = tokio::fs::read_to_string(filename).await?;
    content
        .lines()
        .map(|line| dispatch::execute_offline_line(catalog, line))
        .collect()
}

fn render_outcome(session: &SharedSession, outcome: catalog::CommandOutcome) {
    dispatch::apply_scope_action(session, &outcome.scope_action);
    dispatch::apply_output_state(session, &outcome.output);
    render_snapshot(outcome.output);
}

fn render_snapshot(snapshot: output::OutputSnapshot) {
    if !snapshot.is_empty() {
        let _ = output::print_rendered(&snapshot.render());
    }
}
