use std::env::args;
use std::io::{stdin, IsTerminal};
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use app::{init_logging, load_app_config, login, AppRuntime, SharedSession};
use catalog::{CatalogStore, CommandCatalog};
use cli::{build_cli, execution_mode, split_startup_args, StartupMode};
use commands::build_command_catalog;
use dispatch::{can_execute_offline, execute_line, execute_offline_line};
use errors::AppError;
use presentation::present_command;
use repl::run;
use services::{AppServices, CredentialApprovalSource};
use tokio::fs::read_to_string;
use tokio::runtime::Handle;

mod app;
mod autocomplete;
mod background;
mod build_info;
mod catalog;
mod cli;
mod command_line;
mod commands;
mod config;
mod defaults;
mod diagnostics;
mod dispatch;
mod domain;
mod errors;
mod extensions;
mod files;
mod formatting;
mod json_schema;
mod list_query;
mod models;
mod output;
mod pipe_help;
mod presentation;
mod redirection;
mod repl;
mod services;
mod session_store;
mod suggestions;
mod terminal;
mod theme;
mod tokenizer;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), AppError> {
    let startup_args = split_startup_args(args());
    let matches = build_cli().get_matches_from(startup_args.clap_args);
    diagnostics::set_default(matches.get_flag("debug"));
    let config = load_app_config(&matches)?;
    let catalog = Arc::new(CatalogStore::new(build_command_catalog()));
    let mode = execution_mode(&matches, startup_args.mode);

    match &mode {
        StartupMode::Command(command)
            if can_execute_offline(catalog.snapshot().as_ref(), command) =>
        {
            diagnostics::init();
            if present_command(
                &sessionless(),
                execute_offline_line(catalog.clone(), command),
            )
            .await
            .is_err()
            {
                exit(1);
            }
            return Ok(());
        }
        StartupMode::Script(filename)
            if can_execute_script_offline(catalog.snapshot().as_ref(), filename).await? =>
        {
            diagnostics::init();
            let session = SharedSession::new();
            if !execute_offline_script(catalog.clone(), &session, filename).await? {
                exit(1);
            }
            return Ok(());
        }
        StartupMode::Repl | StartupMode::Command(_) | StartupMode::Script(_) => {}
    }

    init_logging()?;
    let (config, client) = login(config).await?.into_parts();

    let services = Arc::new(AppServices::new(
        client,
        Handle::current(),
        Duration::from_secs(config.background.poll_interval_seconds),
    ));
    let approval_source = match matches.get_one::<PathBuf>("approval_password_file") {
        Some(path) => CredentialApprovalSource::File(path.clone()),
        None if !matches!(mode, StartupMode::Script(_)) && stdin().is_terminal() => {
            CredentialApprovalSource::Prompt
        }
        None => CredentialApprovalSource::Unavailable,
    };
    services
        .gateway()
        .set_credential_approval_source(approval_source);
    let runtime = Arc::new(AppRuntime::new(config, services, catalog));
    let session = SharedSession::new();

    if let StartupMode::Command(command) = mode {
        if present_command(&session, execute_line(runtime.clone(), &session, &command))
            .await
            .is_err()
        {
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

async fn execute_script(
    runtime: Arc<AppRuntime>,
    session: &SharedSession,
    filename: &str,
) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    for line in content.lines() {
        if present_command(session, execute_line(runtime.clone(), session, line))
            .await
            .is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn can_execute_script_offline(
    catalog: &CommandCatalog,
    filename: &str,
) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| can_execute_offline(catalog, line)))
}

async fn execute_offline_script(
    catalog: Arc<CatalogStore>,
    session: &SharedSession,
    filename: &str,
) -> Result<bool, AppError> {
    let content = read_to_string(filename).await?;
    for line in content.lines() {
        if present_command(session, execute_offline_line(catalog.clone(), line))
            .await
            .is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}
