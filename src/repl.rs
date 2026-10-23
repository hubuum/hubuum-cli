mod completion;
use completion::ReplCompleter;
mod state;
pub(crate) use state::SharedReplState;

use std::borrow::Cow;
use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;
use std::thread::spawn;

use crossterm::event::{Event, KeyEvent};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, EditMode, Emacs, FileBackedHistory, KeyCode,
    KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal,
};
use tokio::runtime::Handle;

use crate::app::{reauthenticate, AppRuntime};
use crate::background::BackgroundManager;
use crate::catalog::{CommandOutcome, ScopeAction};
use crate::commands::self_update::startup_notice;
use crate::config::get_config;
use crate::diagnostics;
use crate::dispatch::execute_line;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::files::get_history_file;
use crate::output::{print_rendered, OutputSnapshot};
use crate::presentation::present_command;

const CANCEL_PAGINATION_HOST_COMMAND: &str = "__hubuum_cancel_pagination__";

pub async fn run(app: Arc<AppRuntime>, session: SharedReplState) -> Result<(), AppError> {
    let runtime = Handle::current();
    let join = spawn(move || run_thread(runtime, app, session));

    join.join()
        .map_err(|_| AppError::CommandExecutionError("REPL thread panicked".to_string()))?
}

fn run_thread(
    runtime: Handle,
    app: Arc<AppRuntime>,
    session: SharedReplState,
) -> Result<(), AppError> {
    let _background_guard = BackgroundGuard::new(app.services.background());
    let history = Box::new(
        FileBackedHistory::with_file(1000, get_history_file()?)
            .map_err(|err| AppError::ReplError(err.to_string()))?,
    );
    let completion = app
        .services
        .completion_context(runtime.clone(), app.config.as_ref());
    let completer = Box::new(ReplCompleter::new(app.clone(), session.clone(), completion));
    let menu = Box::new(
        ColumnarMenu::default()
            .with_name("completion_menu")
            .with_marker("")
            .with_only_buffer_difference(false),
    );
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    let edit_mode = Box::new(PaginationEditMode {
        inner: Emacs::new(keybindings),
        session: session.clone(),
    });

    let mut editor = Reedline::create()
        .with_history(history)
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(menu))
        .with_edit_mode(edit_mode)
        .with_quick_completions(true)
        .with_ansi_colors(true);

    let catalog = app.catalog.snapshot();
    let _ = print_rendered(&format!("{}\n", catalog.render_scope_help(&[])));
    if let Some(notice) = startup_notice(&app.config.self_update) {
        let _ = print_rendered(&format!("{notice}\n"));
    }

    loop {
        let prompt = ReplPrompt {
            left: app.prompt(&session),
        };

        let signal = editor
            .read_line(&prompt)
            .map_err(|err| AppError::ReplError(err.to_string()))?;

        if handle_host_signal(&signal, &session) {
            continue;
        }

        match signal {
            Signal::Success(line) => {
                let effective_line = if line.trim().is_empty()
                    && get_config().repl.enter_fetches_next_page
                    && session.next_page_command().is_some()
                {
                    "next".to_string()
                } else {
                    line
                };
                let exit_repl = runtime.block_on(present_command(
                    &session,
                    execute_repl_line(app.clone(), &session, &effective_line),
                )) == Ok(ScopeAction::ExitRepl);
                if exit_repl {
                    break;
                }
            }
            Signal::CtrlD => break,
            Signal::CtrlC => {
                clear_pending_pagination(&session);
                continue;
            }
            _ => continue,
        }
    }

    Ok(())
}

async fn execute_repl_line(
    app: Arc<AppRuntime>,
    session: &SharedReplState,
    line: &str,
) -> Result<CommandOutcome, AppError> {
    let warnings = RefCell::new(Vec::new());
    let result = execute_with_reauthentication(
        || execute_line(app.clone(), session, line),
        |request| {
            let app = app.clone();
            let session = session.clone();
            let warnings = &warnings;
            async move {
                if diagnostics::enabled() && diagnostics::json_output() == Some(true) {
                    warnings
                        .borrow_mut()
                        .extend(reauthentication_warning(&request).warnings);
                } else {
                    print_reauthentication_warning(&request)?;
                }
                session.set_next_page_command(None);
                let client = reauthenticate(app.config.clone()).await?;
                app.services.replace_authenticated_client(client);
                Ok(())
            }
        },
    )
    .await;
    let warnings = warnings.into_inner();
    match result {
        Ok(mut outcome) => {
            outcome.output.warnings.splice(0..0, warnings);
            Ok(outcome)
        }
        Err(error) => Err(error.with_warnings(warnings)),
    }
}

fn print_reauthentication_warning(request: &str) -> Result<(), AppError> {
    print_rendered(&reauthentication_warning(request).render())
}

fn reauthentication_warning(request: &str) -> OutputSnapshot {
    OutputSnapshot {
        warnings: vec![format!(
            "Session expired or the token was revoked during {request}; signing in again."
        )],
        ..OutputSnapshot::default()
    }
}

async fn execute_with_reauthentication<Execute, ExecuteFuture, Reauthenticate, ReauthFuture>(
    mut execute: Execute,
    mut reauthenticate: Reauthenticate,
) -> Result<CommandOutcome, AppError>
where
    Execute: FnMut() -> ExecuteFuture,
    ExecuteFuture: Future<Output = Result<CommandOutcome, AppError>>,
    Reauthenticate: FnMut(String) -> ReauthFuture,
    ReauthFuture: Future<Output = Result<(), AppError>>,
{
    let error = match execute().await {
        Ok(outcome) => return Ok(outcome),
        Err(error) if error.is_unauthorized() => error,
        Err(error) => return Err(error),
    };
    let request = error.unauthorized_request();
    let retry = error
        .reauthentication_retry()
        .unwrap_or(ReauthenticationRetry::Unsafe);

    reauthenticate(request.clone())
        .await
        .map_err(|source| AppError::ReauthenticationFailed {
            request: request.clone(),
            source: Box::new(source),
        })?;

    match retry {
        ReauthenticationRetry::Safe => execute().await,
        ReauthenticationRetry::Unsafe => Err(AppError::CommandNotRetried { request }),
    }
}

fn handle_host_signal(signal: &Signal, session: &SharedReplState) -> bool {
    if let Signal::HostCommand(command) = signal {
        if command == CANCEL_PAGINATION_HOST_COMMAND {
            clear_pending_pagination(session);
        }
        true
    } else {
        false
    }
}

fn clear_pending_pagination(session: &SharedReplState) {
    if session.next_page_command().is_some() {
        session.set_next_page_command(None);
    }
}

struct BackgroundGuard {
    manager: BackgroundManager,
}

struct PaginationEditMode {
    inner: Emacs,
    session: SharedReplState,
}

impl EditMode for PaginationEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = Event::from(event);
        if matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
                ..
            })
        ) && self.session.next_page_command().is_some()
        {
            return ReedlineEvent::ExecuteHostCommand(CANCEL_PAGINATION_HOST_COMMAND.to_string());
        }

        match ReedlineRawEvent::try_from(event) {
            Ok(event) => self.inner.parse_event(event),
            Err(()) => ReedlineEvent::None,
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

impl BackgroundGuard {
    fn new(manager: BackgroundManager) -> Self {
        manager.enable();
        Self { manager }
    }
}

impl Drop for BackgroundGuard {
    fn drop(&mut self) {
        self.manager.disable();
    }
}

struct ReplPrompt {
    left: String,
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.left)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let status = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({status}reverse-search: {}) ",
            history_search.term
        ))
    }
}
