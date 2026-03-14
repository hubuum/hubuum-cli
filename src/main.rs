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
mod commands;
mod config;
mod defaults;
mod dispatch;
mod domain;
mod errors;
mod files;
mod formatting;
mod list_query;
mod models;
mod output;
mod repl;
mod services;
mod tokenizer;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), AppError> {
    let matches = cli::build_cli().get_matches();
    app::init_logging()?;
    let config = app::load_app_config(&matches)?;
    let client = app::login(config.clone()).await?;

    let services = Arc::new(AppServices::new(
        client,
        tokio::runtime::Handle::current(),
        Duration::from_secs(config.background.poll_interval_seconds),
    ));
    let catalog = Arc::new(commands::build_command_catalog());
    let runtime = Arc::new(AppRuntime::new(config, services, catalog));
    let session = SharedSession::new();

    if let Some(command) = matches.get_one::<String>("command") {
        let outcome = dispatch::execute_line(runtime.clone(), &session, command).await;
        if !render_dispatch_result(&session, outcome) {
            std::process::exit(1);
        }
        return Ok(());
    }

    if let Some(filename) = matches.get_one::<String>("source") {
        let outcomes = dispatch::execute_script(runtime.clone(), &session, filename).await;
        let success = match outcomes {
            Ok(outcomes) => {
                for outcome in outcomes {
                    render_outcome(&session, outcome);
                }
                true
            }
            Err(err) => {
                render_snapshot(dispatch::render_error(err));
                false
            }
        };
        if !success {
            std::process::exit(1);
        }
        return Ok(());
    }

    repl::run(runtime, session).await
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

fn render_outcome(session: &SharedSession, outcome: catalog::CommandOutcome) {
    dispatch::apply_scope_action(session, &outcome.scope_action);
    dispatch::apply_output_state(session, &outcome.output);
    render_snapshot(outcome.output);
}

fn render_snapshot(snapshot: output::OutputSnapshot) {
    if !snapshot.is_empty() {
        print!("{}", snapshot.render());
    }
}
