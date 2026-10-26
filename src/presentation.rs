use std::future::Future;

use crate::catalog::{CommandOutcome, ScopeAction};
use crate::diagnostics;
use crate::dispatch::{apply_output_state, apply_scope_action, render_error};
use crate::errors::AppError;
use crate::output::{print_snapshot, RenderFormat};
use crate::redirection::write_output;
use crate::repl::SharedReplState;

/// Execute and present one command, sharing the same diagnostics policy across
/// direct commands, scripts, and the REPL.
pub(crate) async fn present_command(
    session: &SharedReplState,
    operation: impl Future<Output = Result<CommandOutcome, AppError>>,
) -> Result<ScopeAction, ()> {
    let ((mut outcome, mut succeeded, deferred), trace) = diagnostics::capture_command(async {
        let (outcome, succeeded) = match operation.await {
            Ok(outcome) => (outcome, true),
            Err(err) => (
                CommandOutcome {
                    output: render_error(err),
                    ..Default::default()
                },
                false,
            ),
        };
        diagnostics::stage("render", async {
            apply_scope_action(session, &outcome.scope_action);
            apply_output_state(session, &outcome.output);
            // JSON must include the completed report. Its final serialization and
            // sink write consequently happen after command timing has stopped.
            let deferred =
                diagnostics::enabled() && outcome.output.render_format == RenderFormat::Json;
            let written = deferred || emit(&outcome);
            (outcome, succeeded && written, deferred)
        })
        .await
    })
    .await;

    if deferred {
        outcome.output.debug = trace.and_then(|trace| trace.snapshot());
        succeeded &= emit(&outcome);
    } else if let Some(report) = trace.and_then(|trace| trace.render()) {
        eprint!("{report}");
    }
    if succeeded {
        Ok(outcome.scope_action)
    } else {
        Err(())
    }
}

fn emit(outcome: &CommandOutcome) -> bool {
    let result = match &outcome.redirect {
        Some(redirect) => outcome
            .output
            .print_stderr()
            .and_then(|()| write_output(&outcome.output, redirect)),
        None if outcome.output.is_empty() => Ok(()),
        None => print_snapshot(&outcome.output),
    };
    match result {
        Ok(()) => true,
        Err(err) => {
            let mut error = render_error(err);
            error.render_format = outcome.output.render_format;
            error.debug = outcome.output.debug.clone();
            let _ = print_snapshot(&error);
            false
        }
    }
}
