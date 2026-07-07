use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use crossterm::event::{Event, KeyEvent};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, Completer, EditMode, Emacs, FileBackedHistory,
    KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal,
    Span, Suggestion,
};

use crate::app::{AppRuntime, SharedSession};
use crate::autocomplete::{complete_sort_clause, complete_where_clause};
use crate::catalog::{CommandOutcome, CompletionSpec, OptionSpec, ScopeAction};
use crate::dispatch;
use crate::errors::AppError;
use crate::files::get_history_file;

const CANCEL_PAGINATION_HOST_COMMAND: &str = "__hubuum_cancel_pagination__";

pub async fn run(app: Arc<AppRuntime>, session: SharedSession) -> Result<(), AppError> {
    let runtime = tokio::runtime::Handle::current();
    let join = std::thread::spawn(move || run_thread(runtime, app, session));

    join.join()
        .map_err(|_| AppError::CommandExecutionError("REPL thread panicked".to_string()))?
}

fn run_thread(
    runtime: tokio::runtime::Handle,
    app: Arc<AppRuntime>,
    session: SharedSession,
) -> Result<(), AppError> {
    let _background_guard = BackgroundGuard::new(app.services.background());
    let history = Box::new(
        FileBackedHistory::with_file(1000, get_history_file()?)
            .map_err(|err| AppError::ReplError(err.to_string()))?,
    );
    let completion = app
        .services
        .completion_context(runtime.clone(), app.config.as_ref());
    let completer = Box::new(ReplCompleter {
        app: app.clone(),
        session: session.clone(),
        completion,
    });
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

    let _ = crate::output::print_rendered(&format!("{}\n", app.catalog.render_scope_help(&[])));

    loop {
        let prompt = ReplPrompt {
            left: app.prompt(&session),
        };

        let signal = editor
            .read_line(&prompt)
            .map_err(|err| AppError::ReplError(err.to_string()))?;

        match signal {
            Signal::Success(line) => {
                if line == CANCEL_PAGINATION_HOST_COMMAND {
                    clear_pending_pagination(&session);
                    continue;
                }

                let effective_line = if line.trim().is_empty()
                    && crate::config::get_config().repl.enter_fetches_next_page
                    && session.next_page_command().is_some()
                {
                    "next".to_string()
                } else {
                    line
                };
                let result = runtime.block_on(dispatch::execute_line(
                    app.clone(),
                    &session,
                    &effective_line,
                ));
                match result {
                    Ok(outcome) => {
                        let exit_repl = outcome.scope_action == ScopeAction::ExitRepl;
                        if let Err(err) = apply_outcome(&session, outcome) {
                            let _ = crate::output::print_rendered(
                                &dispatch::render_error(err).render(),
                            );
                        }
                        if exit_repl {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ =
                            crate::output::print_rendered(&dispatch::render_error(err).render());
                    }
                }
            }
            Signal::CtrlD => break,
            Signal::CtrlC => {
                clear_pending_pagination(&session);
                continue;
            }
        }
    }

    Ok(())
}

fn clear_pending_pagination(session: &SharedSession) {
    if session.next_page_command().is_some() {
        session.set_next_page_command(None);
    }
}

struct BackgroundGuard {
    manager: crate::background::BackgroundManager,
}

struct PaginationEditMode {
    inner: Emacs,
    session: SharedSession,
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
    fn new(manager: crate::background::BackgroundManager) -> Self {
        manager.enable();
        Self { manager }
    }
}

impl Drop for BackgroundGuard {
    fn drop(&mut self) {
        self.manager.disable();
    }
}

fn apply_outcome(session: &SharedSession, outcome: CommandOutcome) -> Result<(), AppError> {
    dispatch::apply_scope_action(session, &outcome.scope_action);
    dispatch::apply_output_state(session, &outcome.output);
    if let Some(redirect) = outcome.redirect {
        crate::redirection::write_output(&outcome.output, &redirect)?;
    } else if !outcome.output.is_empty() {
        crate::output::print_rendered(&outcome.output.render())?;
    }
    Ok(())
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

struct ReplCompleter {
    app: Arc<AppRuntime>,
    session: SharedSession,
    completion: crate::services::CompletionContext,
}

impl Completer for ReplCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let prefix_line = &line[..safe_prefix_end(line, pos)];
        if let Some(suggestions) = self.quoted_where_suggestions(prefix_line, pos) {
            return suggestions;
        }
        if let Some(suggestions) = self.pipe_suggestions(prefix_line, pos) {
            return suggestions;
        }
        if let Some((prefix, replacement_start)) =
            crate::redirection::redirect_completion_context(prefix_line, pos)
        {
            return crate::autocomplete::file_paths(&self.completion, prefix, &[])
                .into_iter()
                .map(|value| dynamic_value_suggestion(value, replacement_start, pos))
                .collect();
        }
        let ends_with_space = prefix_line.ends_with(' ');
        let (start, word) = prefix_line
            .rsplit_once(char::is_whitespace)
            .map_or((0, prefix_line), |(left, right)| (left.len() + 1, right));

        let Some(parts) = shlex::split(prefix_line) else {
            return Vec::new();
        };

        if parts.is_empty() {
            return self.scope_suggestions(start, word, &[], ends_with_space);
        }

        let scope = self.session.scope();

        if parts[0] == "help" || parts[0] == "?" {
            return self.scope_suggestions(start, word, &parts[1..], ends_with_space);
        }

        if let Ok(resolved) = self.app.catalog.resolve_command(&scope, &parts) {
            let options = &resolved.command.options;
            let options_seen: Vec<String> = parts
                .iter()
                .filter(|part| part.starts_with('-'))
                .map(|part| option_token_name(part).to_string())
                .collect();

            if let Some(suggestions) = self.where_clause_suggestions(
                &resolved.command_path,
                &parts,
                start,
                pos,
                word,
                ends_with_space,
            ) {
                return suggestions;
            }

            if let Some(suggestions) = self.sort_clause_suggestions(
                &resolved.command_path,
                &parts,
                start,
                pos,
                word,
                ends_with_space,
            ) {
                return suggestions;
            }

            if let Some(suggestions) = self.id_value_suggestions(
                &resolved.command_path,
                &parts,
                start,
                pos,
                word,
                ends_with_space,
            ) {
                return suggestions;
            }

            if let Some(last) = parts.last() {
                if let Some(context) =
                    option_value_context(&parts, start, pos, word, ends_with_space)
                {
                    if let Some(option) = options.iter().find(|option| {
                        option.short.as_deref() == Some(context.option_name)
                            || option.long.as_deref() == Some(context.option_name)
                    }) {
                        if let CompletionSpec::Dynamic(completion) = option.completion.clone() {
                            return completion(&self.completion, context.prefix, &parts)
                                .into_iter()
                                .map(|value| {
                                    dynamic_value_suggestion(
                                        value,
                                        context.replacement_start,
                                        context.replacement_end,
                                    )
                                })
                                .collect();
                        }
                    }
                }

                if last.starts_with('-') || prefix_line.ends_with(' ') {
                    return options
                        .iter()
                        .filter(|option| {
                            option.repeatable
                                || (!options_seen
                                    .iter()
                                    .any(|seen| option.short.as_deref() == Some(seen.as_str()))
                                    && !options_seen
                                        .iter()
                                        .any(|seen| option.long.as_deref() == Some(seen.as_str())))
                        })
                        .filter_map(|option| option_suggestion(option, word, start, pos))
                        .collect();
                }
            }
        }

        self.scope_suggestions(start, word, &parts, ends_with_space)
    }
}

impl ReplCompleter {
    fn quoted_where_suggestions(&self, prefix_line: &str, pos: usize) -> Option<Vec<Suggestion>> {
        let quoted = quoted_where_context(prefix_line)?;
        let parts = shlex::split(quoted.command_prefix)?;
        let scope = self.session.scope();
        let resolved = self.app.catalog.resolve_command(&scope, &parts).ok()?;
        let replacement_start = quoted.start
            + clause_active_token_offset(quoted.clause_prefix, quoted.clause_ends_with_space);
        Some(
            complete_where_clause(
                &self.completion,
                &resolved.command_path,
                &parts,
                quoted.clause_prefix,
                quoted.clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                where_suggestion(
                    candidate.value,
                    replacement_start,
                    pos,
                    candidate.description,
                    candidate.append_whitespace,
                )
            })
            .collect(),
        )
    }

    fn where_clause_suggestions(
        &self,
        command_path: &[String],
        parts: &[String],
        start: usize,
        pos: usize,
        word: &str,
        ends_with_space: bool,
    ) -> Option<Vec<Suggestion>> {
        let context =
            clause_option_context(parts, "--where", 3, start, pos, word, ends_with_space)?;
        if context.is_complete && !ends_with_space {
            return Some(vec![suggestion_with_whitespace(
                context.prefix.to_string(),
                start,
                pos,
                None,
                true,
            )]);
        }
        if context.is_complete && ends_with_space {
            return None;
        }

        Some(
            complete_where_clause(
                &self.completion,
                command_path,
                parts,
                &context.clause_prefix,
                context.clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                where_suggestion(
                    candidate.value,
                    context.replacement_start,
                    pos,
                    candidate.description,
                    candidate.append_whitespace,
                )
            })
            .collect(),
        )
    }

    fn sort_clause_suggestions(
        &self,
        command_path: &[String],
        parts: &[String],
        start: usize,
        pos: usize,
        word: &str,
        ends_with_space: bool,
    ) -> Option<Vec<Suggestion>> {
        let context = clause_option_context(parts, "--sort", 2, start, pos, word, ends_with_space)?;
        if context.is_complete && !ends_with_space {
            return Some(vec![suggestion_with_whitespace(
                context.prefix.to_string(),
                start,
                pos,
                None,
                true,
            )]);
        }
        if context.is_complete && ends_with_space {
            return None;
        }

        Some(
            complete_sort_clause(
                &self.completion,
                command_path,
                &context.clause_prefix,
                context.clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                suggestion_with_whitespace(
                    candidate.value,
                    context.replacement_start,
                    pos,
                    candidate.description,
                    candidate.append_whitespace,
                )
            })
            .collect(),
        )
    }

    fn pipe_suggestions(&self, prefix_line: &str, pos: usize) -> Option<Vec<Suggestion>> {
        let context = pipe_completion_context(prefix_line, pos)?;
        if context.kind == PipeCompletionKind::Stage {
            return Some(
                PIPE_STAGES
                    .iter()
                    .copied()
                    .filter(|stage| stage.starts_with(context.prefix))
                    .map(|stage| {
                        let value = if context.needs_leading_space {
                            format!(" {stage}")
                        } else {
                            stage.to_string()
                        };
                        let display_override =
                            context.needs_leading_space.then(|| stage.to_string());
                        pipe_stage_suggestion(
                            value,
                            context.replacement_start,
                            pos,
                            display_override,
                        )
                    })
                    .collect(),
            );
        }

        let command_parts = shlex::split(context.command_prefix.trim())?;
        let scope = self.session.scope();
        let resolved = self
            .app
            .catalog
            .resolve_command(&scope, &command_parts)
            .ok()?;
        let fields =
            projection_fields_for_command(&self.completion, &resolved.command_path, &command_parts);
        if fields.is_empty() {
            return None;
        }

        Some(
            fields
                .into_iter()
                .filter(|field| field.starts_with(context.prefix))
                .map(|field| {
                    let value = if context.needs_leading_space {
                        format!(" {field}")
                    } else {
                        field.clone()
                    };
                    let display_override = context.needs_leading_space.then_some(field);
                    projection_suggestion(value, context.replacement_start, pos, display_override)
                })
                .collect(),
        )
    }

    fn id_value_suggestions(
        &self,
        command_path: &[String],
        parts: &[String],
        start: usize,
        pos: usize,
        word: &str,
        ends_with_space: bool,
    ) -> Option<Vec<Suggestion>> {
        let context =
            id_completion_context(command_path, parts, start, pos, word, ends_with_space)?;
        let suggestions = match context.kind {
            IdCompletionKind::LocalJob => self.local_job_id_suggestions(
                context.prefix,
                context.replacement_start,
                context.replacement_end,
            ),
            IdCompletionKind::Task => self.task_id_suggestions(
                context.prefix,
                context.replacement_start,
                context.replacement_end,
            ),
            IdCompletionKind::ImportTask => self.import_task_id_suggestions(
                context.prefix,
                context.replacement_start,
                context.replacement_end,
            ),
        };
        Some(suggestions)
    }

    fn local_job_id_suggestions(&self, prefix: &str, start: usize, end: usize) -> Vec<Suggestion> {
        self.app
            .services
            .background()
            .list_jobs()
            .into_iter()
            .filter_map(|job| {
                let value = job.id.to_string();
                value.starts_with(prefix).then(|| {
                    let mut details = vec![
                        format!("task {}", job.task_id),
                        job.state,
                        job.status,
                        job.label,
                    ];
                    if let Some(summary) = job.summary.filter(|summary| !summary.is_empty()) {
                        details.push(summary);
                    }
                    suggestion_with_whitespace(value, start, end, Some(details.join("  ")), true)
                })
            })
            .collect()
    }

    fn task_id_suggestions(&self, prefix: &str, start: usize, end: usize) -> Vec<Suggestion> {
        self.completion
            .task_ids(prefix)
            .into_iter()
            .map(|item| suggestion_with_whitespace(item.value, start, end, item.description, true))
            .collect()
    }

    fn import_task_id_suggestions(
        &self,
        prefix: &str,
        start: usize,
        end: usize,
    ) -> Vec<Suggestion> {
        self.completion
            .import_task_ids(prefix)
            .into_iter()
            .map(|item| suggestion_with_whitespace(item.value, start, end, item.description, true))
            .collect()
    }

    fn scope_suggestions(
        &self,
        start: usize,
        word: &str,
        parts: &[String],
        ends_with_space: bool,
    ) -> Vec<Suggestion> {
        let scope = self.session.scope();
        let context_parts = completion_context_parts(parts, ends_with_space);
        let scope_words = if context_parts.is_empty() {
            self.app.catalog.list_words(&scope)
        } else if let Some(scope_spec) = self.app.catalog.resolve_scope(&scope, context_parts) {
            scope_spec
                .commands
                .keys()
                .chain(scope_spec.scopes.keys())
                .cloned()
                .collect()
        } else {
            self.app.catalog.list_words(&scope)
        };

        let mut scope_words = scope_words;
        scope_words.push("?".to_string());
        if !scope.is_empty() {
            scope_words.push("..".to_string());
        }

        scope_words
            .into_iter()
            .filter(|value| value.starts_with(word))
            .map(|value| suggestion(value, start, start + word.len(), None))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdCompletionKind {
    LocalJob,
    Task,
    ImportTask,
}

#[derive(Debug, Clone, Copy)]
struct IdCompletionContext<'a> {
    kind: IdCompletionKind,
    prefix: &'a str,
    replacement_start: usize,
    replacement_end: usize,
}

fn id_completion_context<'a>(
    command_path: &[String],
    parts: &'a [String],
    start: usize,
    pos: usize,
    word: &'a str,
    ends_with_space: bool,
) -> Option<IdCompletionContext<'a>> {
    let kind = id_completion_kind(command_path)?;
    let option_names = id_completion_option_names(command_path, kind);

    if !ends_with_space {
        if let Some((option_name, prefix)) = word.split_once('=') {
            if option_names.contains(&option_name) {
                return Some(IdCompletionContext {
                    kind,
                    prefix,
                    replacement_start: start + option_name.len() + 1,
                    replacement_end: pos,
                });
            }
        }
    }

    if ends_with_space {
        if let Some(last) = parts.last() {
            if option_names.contains(&last.as_str()) {
                return Some(IdCompletionContext {
                    kind,
                    prefix: "",
                    replacement_start: pos,
                    replacement_end: pos,
                });
            }
        }
    } else if let Some(previous) = parts.iter().rev().nth(1) {
        if option_names.contains(&previous.as_str()) {
            return Some(IdCompletionContext {
                kind,
                prefix: word,
                replacement_start: start,
                replacement_end: pos,
            });
        }
    }

    if is_completing_positional_id(command_path, parts, ends_with_space) {
        return Some(IdCompletionContext {
            kind,
            prefix: if ends_with_space { "" } else { word },
            replacement_start: if ends_with_space { pos } else { start },
            replacement_end: pos,
        });
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OptionValueContext<'a> {
    option_name: &'a str,
    prefix: &'a str,
    replacement_start: usize,
    replacement_end: usize,
}

fn option_value_context<'a>(
    parts: &'a [String],
    start: usize,
    pos: usize,
    word: &'a str,
    ends_with_space: bool,
) -> Option<OptionValueContext<'a>> {
    if ends_with_space {
        let last = parts.last()?;
        if last.starts_with('-') {
            return Some(OptionValueContext {
                option_name: option_token_name(last),
                prefix: "",
                replacement_start: pos,
                replacement_end: pos,
            });
        }
        return None;
    }

    if let Some((option_name, prefix)) = word.split_once('=') {
        if option_name.starts_with('-') {
            return Some(OptionValueContext {
                option_name,
                prefix,
                replacement_start: start + option_name.len() + 1,
                replacement_end: pos,
            });
        }
    }

    let previous = parts.iter().rev().nth(1)?;
    previous.starts_with('-').then_some(OptionValueContext {
        option_name: option_token_name(previous),
        prefix: word,
        replacement_start: start,
        replacement_end: pos,
    })
}

fn option_token_name(token: &str) -> &str {
    token.split_once('=').map_or(token, |(name, _)| name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClauseOptionContext {
    clause_prefix: String,
    clause_ends_with_space: bool,
    prefix: String,
    replacement_start: usize,
    is_complete: bool,
}

fn clause_option_context(
    parts: &[String],
    option_name: &str,
    value_count: usize,
    start: usize,
    pos: usize,
    word: &str,
    ends_with_space: bool,
) -> Option<ClauseOptionContext> {
    let (option_index, inline_prefix) =
        parts.iter().enumerate().rev().find_map(|(index, part)| {
            if part == option_name {
                return Some((index, None));
            }
            part.strip_prefix(&format!("{option_name}="))
                .map(|value| (index, Some(value)))
        })?;

    let mut clause_parts = Vec::new();
    if let Some(value) = inline_prefix.filter(|value| !value.is_empty()) {
        clause_parts.push(value.to_string());
    }
    clause_parts.extend(parts[option_index + 1..].iter().cloned());

    let prefix = if ends_with_space {
        String::new()
    } else if inline_prefix.is_some() && option_index == parts.len().saturating_sub(1) {
        inline_prefix.unwrap_or_default().to_string()
    } else {
        word.to_string()
    };

    let replacement_start =
        if !ends_with_space && inline_prefix.is_some() && option_index == parts.len() - 1 {
            start + option_name.len() + 1
        } else if ends_with_space && clause_parts.is_empty() {
            pos
        } else {
            start
        };

    let mut clause_prefix = clause_parts.join(" ");
    if ends_with_space && !clause_prefix.is_empty() {
        clause_prefix.push(' ');
    }

    Some(ClauseOptionContext {
        clause_prefix,
        clause_ends_with_space: ends_with_space,
        prefix,
        replacement_start,
        is_complete: clause_parts.len() >= value_count,
    })
}

fn id_completion_kind(command_path: &[String]) -> Option<IdCompletionKind> {
    match command_path {
        [scope, command]
            if (scope == "jobs" || scope == "bg")
                && matches!(command.as_str(), "show" | "output" | "forget") =>
        {
            Some(IdCompletionKind::LocalJob)
        }
        [scope, command] if (scope == "jobs" || scope == "bg") && command == "watch" => {
            Some(IdCompletionKind::Task)
        }
        [scope, command]
            if scope == "task" && matches!(command.as_str(), "show" | "events" | "output") =>
        {
            Some(IdCompletionKind::Task)
        }
        [scope, command] if scope == "import" && matches!(command.as_str(), "show" | "results") => {
            Some(IdCompletionKind::ImportTask)
        }
        _ => None,
    }
}

fn id_completion_option_names(
    command_path: &[String],
    kind: IdCompletionKind,
) -> &'static [&'static str] {
    match (kind, command_path) {
        (IdCompletionKind::Task, [scope, command])
            if (scope == "jobs" || scope == "bg") && command == "watch" =>
        {
            &["--task", "-t"]
        }
        (IdCompletionKind::LocalJob, _)
        | (IdCompletionKind::Task, _)
        | (IdCompletionKind::ImportTask, _) => &["--id", "-i"],
    }
}

fn is_completing_positional_id(
    command_path: &[String],
    parts: &[String],
    ends_with_space: bool,
) -> bool {
    if parts.len() < command_path.len() {
        return false;
    }
    if parts
        .iter()
        .skip(command_path.len())
        .any(|part| part.starts_with('-'))
    {
        return false;
    }
    let positional_count = parts.len().saturating_sub(command_path.len());
    (ends_with_space && positional_count == 0) || (!ends_with_space && positional_count == 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipeCompletionKind {
    Stage,
    Field,
}

#[derive(Debug, Clone, Copy)]
struct PipeCompletionContext<'a> {
    command_prefix: &'a str,
    kind: PipeCompletionKind,
    prefix: &'a str,
    replacement_start: usize,
    needs_leading_space: bool,
}

const PIPE_STAGES: &[&str] = &[
    "grep", "F", "V", "K", "?", "reject", "P", "columns", "S", "sort", "G", "A", "L", "head",
    "tail", "C", "count", "U", "Z", "JQ", "VALUE", "VAL",
];

fn pipe_completion_context(prefix_line: &str, pos: usize) -> Option<PipeCompletionContext<'_>> {
    let pipe_index = last_unquoted_pipe(prefix_line)?;
    let command_prefix = &prefix_line[..pipe_index];
    let pipe_prefix = &prefix_line[pipe_index + 1..];
    let pipe_parts = shlex::split(pipe_prefix)?;
    let ends_with_space = prefix_line.ends_with(char::is_whitespace);
    let (word_start, word) = if ends_with_space {
        (pos, "")
    } else {
        prefix_line
            .rsplit_once(char::is_whitespace)
            .map_or((pipe_index + 1, pipe_prefix), |(left, right)| {
                (left.len() + 1, right)
            })
    };

    let leading_pipe_space = pipe_prefix.chars().next().is_some_and(char::is_whitespace);

    if pipe_parts.is_empty() {
        return Some(PipeCompletionContext {
            command_prefix,
            kind: PipeCompletionKind::Stage,
            prefix: "",
            replacement_start: pos,
            needs_leading_space: !leading_pipe_space,
        });
    }

    let stage = pipe_parts.first()?;
    if pipe_parts.len() == 1 && !ends_with_space {
        if field_completion_stage(stage) {
            return Some(PipeCompletionContext {
                command_prefix,
                kind: PipeCompletionKind::Field,
                prefix: "",
                replacement_start: pos,
                needs_leading_space: true,
            });
        }

        return Some(PipeCompletionContext {
            command_prefix,
            kind: PipeCompletionKind::Stage,
            prefix: word,
            replacement_start: word_start,
            needs_leading_space: false,
        });
    }

    if !field_completion_stage(stage)
        || !should_complete_stage_field(stage, &pipe_parts, ends_with_space)
    {
        return None;
    }

    let prefix = if field_uses_comma_segments(stage) {
        let comma_offset = word.rfind(',').map(|index| index + 1).unwrap_or(0);
        return Some(PipeCompletionContext {
            command_prefix,
            kind: PipeCompletionKind::Field,
            prefix: &word[comma_offset..],
            replacement_start: word_start + comma_offset,
            needs_leading_space: false,
        });
    } else {
        word
    };

    Some(PipeCompletionContext {
        command_prefix,
        kind: PipeCompletionKind::Field,
        prefix,
        replacement_start: word_start,
        needs_leading_space: false,
    })
}

fn field_completion_stage(stage: &str) -> bool {
    matches!(
        stage,
        "P" | "columns"
            | "VAL"
            | "VALUE"
            | "S"
            | "sort"
            | "grep"
            | "F"
            | "V"
            | "K"
            | "?"
            | "reject"
            | "G"
            | "U"
            | "A"
    )
}

fn should_complete_stage_field(stage: &str, pipe_parts: &[String], ends_with_space: bool) -> bool {
    match stage {
        "P" | "columns" => pipe_parts.len() >= 2 || ends_with_space,
        "VAL" | "VALUE" | "S" | "sort" | "grep" | "F" | "V" | "K" | "?" | "reject" | "G" | "U"
        | "A" => {
            (pipe_parts.len() == 1 && ends_with_space)
                || (pipe_parts.len() == 2 && !ends_with_space)
        }
        _ => false,
    }
}

fn field_uses_comma_segments(stage: &str) -> bool {
    matches!(stage, "P" | "columns")
}

fn last_unquoted_pipe(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut last_pipe = None;

    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '|' => last_pipe = Some(index),
            None => {}
        }
    }

    last_pipe
}

fn projection_fields_for_command(
    ctx: &crate::services::CompletionContext,
    command_path: &[String],
    command_parts: &[String],
) -> Vec<String> {
    if matches!(command_path, [scope, command] if scope == "object" && command == "list") {
        return object_list_projection_fields(ctx, command_parts);
    }

    Vec::new()
}

fn object_list_projection_fields(
    ctx: &crate::services::CompletionContext,
    command_parts: &[String],
) -> Vec<String> {
    let mut fields = BTreeSet::from([
        "id".to_string(),
        "Name".to_string(),
        "Description".to_string(),
        "Namespace".to_string(),
        "Class".to_string(),
        "Data".to_string(),
        "Created".to_string(),
        "Updated".to_string(),
        "name".to_string(),
        "description".to_string(),
        "namespace".to_string(),
        "class".to_string(),
        "data".to_string(),
        "created_at".to_string(),
        "updated_at".to_string(),
    ]);

    if let Some(class_name) = class_name_from_command_parts(command_parts) {
        if let Some(columns) = crate::config::get_config()
            .output
            .object_list_class_columns
            .get(&class_name)
        {
            fields.extend(columns.iter().cloned());
        }

        if let Some(Some(schema)) = ctx.class_schema(&class_name) {
            for path in crate::json_schema::schema_paths(&schema, true) {
                fields.insert(format!("data.{path}"));
            }
        }
    }

    fields.into_iter().collect()
}

fn class_name_from_command_parts(parts: &[String]) -> Option<String> {
    parts
        .windows(2)
        .find(|pair| pair[0] == "--class" || pair[0] == "-c")
        .map(|pair| pair[1].clone())
}

fn completion_context_parts(parts: &[String], ends_with_space: bool) -> &[String] {
    if ends_with_space || parts.is_empty() {
        parts
    } else {
        &parts[..parts.len().saturating_sub(1)]
    }
}

#[cfg(test)]
fn is_completing_option_value(parts: &[String], ends_with_space: bool) -> bool {
    !ends_with_space
        && parts.len() >= 2
        && parts
            .get(parts.len().saturating_sub(2))
            .is_some_and(|part| part.starts_with('-'))
}

fn suggestion(value: String, start: usize, end: usize, description: Option<String>) -> Suggestion {
    suggestion_with_whitespace(value, start, end, description, true)
}

fn dynamic_value_suggestion(value: String, start: usize, end: usize) -> Suggestion {
    let append_whitespace = !value.ends_with(std::path::MAIN_SEPARATOR);
    suggestion_with_whitespace(value, start, end, None, append_whitespace)
}

fn suggestion_with_whitespace(
    value: String,
    start: usize,
    end: usize,
    description: Option<String>,
    append_whitespace: bool,
) -> Suggestion {
    Suggestion {
        value,
        description,
        style: None,
        extra: None,
        match_indices: Some(Vec::new()),
        span: Span { start, end },
        display_override: None,
        append_whitespace,
    }
}

fn where_suggestion(
    value: String,
    start: usize,
    end: usize,
    description: Option<String>,
    append_whitespace: bool,
) -> Suggestion {
    let display_override = description
        .as_deref()
        .filter(|description| {
            matches!(
                *description,
                "no schema" | "no schema match" | "type path manually"
            )
        })
        .map(str::to_string);

    Suggestion {
        display_override,
        ..suggestion_with_whitespace(value, start, end, description, append_whitespace)
    }
}

fn projection_suggestion(
    value: String,
    start: usize,
    end: usize,
    display_override: Option<String>,
) -> Suggestion {
    Suggestion {
        display_override,
        ..suggestion_with_whitespace(value, start, end, Some("output field".to_string()), true)
    }
}

fn pipe_stage_suggestion(
    value: String,
    start: usize,
    end: usize,
    display_override: Option<String>,
) -> Suggestion {
    Suggestion {
        display_override,
        ..suggestion_with_whitespace(value, start, end, Some("pipe stage".to_string()), true)
    }
}

fn option_suggestion(
    option: &OptionSpec,
    word: &str,
    start: usize,
    end: usize,
) -> Option<Suggestion> {
    let short_matches = option
        .short
        .as_deref()
        .is_some_and(|name| name.starts_with(word));
    let long_matches = option
        .long
        .as_deref()
        .is_some_and(|name| name.starts_with(word));

    if !short_matches && !long_matches {
        return None;
    }

    let value = preferred_option_alias(option, word, short_matches, long_matches)?;
    let description = option_description(option, value);

    Some(suggestion(value.to_string(), start, end, Some(description)))
}

fn preferred_option_alias<'a>(
    option: &'a OptionSpec,
    word: &str,
    short_matches: bool,
    long_matches: bool,
) -> Option<&'a str> {
    if word.starts_with("--") {
        return option.long.as_deref().or(option.short.as_deref());
    }

    if short_matches && (!long_matches || word.starts_with('-')) {
        return option.short.as_deref();
    }

    if long_matches {
        return option.long.as_deref();
    }

    option.short.as_deref()
}

fn option_description(option: &OptionSpec, inserted: &str) -> String {
    let mut details = Vec::new();

    let aliases: Vec<&str> = [option.short.as_deref(), option.long.as_deref()]
        .into_iter()
        .flatten()
        .filter(|alias| *alias != inserted)
        .collect();
    if !aliases.is_empty() {
        details.push(aliases.join(", "));
    }

    if !option.flag {
        details.push(format!("<{}>", option.field_type_help));
    }

    details.push(option.help.clone());
    details.join("  ")
}

fn safe_prefix_end(line: &str, pos: usize) -> usize {
    let mut end = pos.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    end
}

struct QuotedWhereContext<'a> {
    command_prefix: &'a str,
    clause_prefix: &'a str,
    clause_ends_with_space: bool,
    start: usize,
}

fn quoted_where_context(prefix_line: &str) -> Option<QuotedWhereContext<'_>> {
    let where_index = prefix_line.rfind("--where")?;
    let after_option = &prefix_line[where_index + "--where".len()..];
    let spaces = after_option.len() - after_option.trim_start().len();
    let after_spaces = &after_option[spaces..];
    let quote = after_spaces.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let quoted = &after_spaces[quote.len_utf8()..];
    if quoted.contains(quote) {
        return None;
    }

    let start = where_index + "--where".len() + spaces + quote.len_utf8();
    Some(QuotedWhereContext {
        command_prefix: &prefix_line[..where_index + "--where".len()],
        clause_prefix: quoted,
        clause_ends_with_space: quoted.ends_with(' '),
        start,
    })
}

fn clause_active_token_offset(clause: &str, ends_with_space: bool) -> usize {
    if ends_with_space {
        return clause.len();
    }

    clause
        .rfind(char::is_whitespace)
        .map(|index| index + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use crossterm::event::{
        Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
        KeyModifiers as CrosstermKeyModifiers,
    };
    use reedline::{default_emacs_keybindings, EditMode, Emacs, ReedlineEvent, ReedlineRawEvent};

    use crate::app::SharedSession;
    use crate::catalog::{CompletionSpec, OptionSpec};

    use super::{
        clause_active_token_offset, clause_option_context, completion_context_parts,
        id_completion_context, is_completing_option_value, option_suggestion, option_value_context,
        pipe_completion_context, quoted_where_context, safe_prefix_end, where_suggestion,
        IdCompletionKind, PaginationEditMode, PipeCompletionKind, CANCEL_PAGINATION_HOST_COMMAND,
    };
    use crate::json_schema::schema_paths;

    #[test]
    fn esc_cancels_only_when_pagination_is_pending() {
        let session = SharedSession::new();
        let mut edit_mode = PaginationEditMode {
            inner: Emacs::new(default_emacs_keybindings()),
            session: session.clone(),
        };

        assert_eq!(edit_mode.parse_event(esc_event()), ReedlineEvent::Esc);

        session.set_next_page_command(Some("next --cursor abc".to_string()));
        assert_eq!(
            edit_mode.parse_event(esc_event()),
            ReedlineEvent::ExecuteHostCommand(CANCEL_PAGINATION_HOST_COMMAND.to_string())
        );
    }

    #[test]
    fn completion_context_uses_parent_path_for_partial_word() {
        let parts = vec!["namespace".to_string(), "mod".to_string()];
        let context = completion_context_parts(&parts, false);
        assert_eq!(context, &parts[..1]);
    }

    #[test]
    fn completion_context_keeps_full_path_after_space() {
        let parts = vec!["namespace".to_string()];
        let context = completion_context_parts(&parts, true);
        assert_eq!(context, &parts[..]);
    }

    #[test]
    fn option_value_completion_stops_after_trailing_space() {
        let parts = vec![
            "namespace".to_string(),
            "modify".to_string(),
            "--name".to_string(),
            "UiO-wide".to_string(),
        ];
        assert!(!is_completing_option_value(&parts, true));
    }

    #[test]
    fn option_value_completion_runs_while_typing_value() {
        let parts = vec![
            "namespace".to_string(),
            "modify".to_string(),
            "--name".to_string(),
            "Ui".to_string(),
        ];
        assert!(is_completing_option_value(&parts, false));
    }

    #[test]
    fn option_value_context_accepts_inline_values() {
        let parts = vec![
            "task".to_string(),
            "show".to_string(),
            "--id=12".to_string(),
        ];
        let context = option_value_context(
            &parts,
            "task show ".len(),
            "task show --id=12".len(),
            "--id=12",
            false,
        )
        .expect("inline option value context");

        assert_eq!(context.option_name, "--id");
        assert_eq!(context.prefix, "12");
        assert_eq!(context.replacement_start, "task show --id=".len());
    }

    #[test]
    fn safe_prefix_end_clamps_past_end_positions() {
        assert_eq!(safe_prefix_end("", 2), 0);
        assert_eq!(safe_prefix_end("user list", 99), "user list".len());
    }

    #[test]
    fn safe_prefix_end_rewinds_to_char_boundary() {
        let value = "aø";
        assert_eq!(safe_prefix_end(value, 2), 1);
        assert_eq!(safe_prefix_end(value, 3), 3);
    }

    #[test]
    fn option_suggestion_renders_one_entry_for_short_and_long_aliases() {
        let option = test_option(Some("-n"), Some("--name"), false, "Name of the namespace");
        let suggestion = option_suggestion(&option, "-", 0, 1).expect("suggestion");

        assert_eq!(suggestion.value, "-n");
        assert_eq!(
            suggestion.description.as_deref(),
            Some("--name  <string>  Name of the namespace")
        );
    }

    #[test]
    fn option_suggestion_prefers_long_alias_for_long_prefixes() {
        let option = test_option(Some("-n"), Some("--name"), false, "Name of the namespace");
        let suggestion = option_suggestion(&option, "--n", 0, 3).expect("suggestion");

        assert_eq!(suggestion.value, "--name");
        assert_eq!(
            suggestion.description.as_deref(),
            Some("-n  <string>  Name of the namespace")
        );
    }

    #[test]
    fn quoted_where_context_extracts_open_clause_contents() {
        let context = quoted_where_context("namespace list --where 'name ic")
            .expect("quoted where should be detected");

        assert_eq!(context.command_prefix, "namespace list --where");
        assert_eq!(context.clause_prefix, "name ic");
        assert_eq!(context.start, "namespace list --where '".len());
    }

    #[test]
    fn quoted_where_context_ignores_closed_quotes() {
        assert!(quoted_where_context("namespace list --where 'name icontains foo'").is_none());
    }

    #[test]
    fn clause_active_token_offset_tracks_current_clause_word() {
        assert_eq!(clause_active_token_offset("json_data.contact", false), 0);
        assert_eq!(
            clause_active_token_offset("json_data.contact ", true),
            "json_data.contact ".len()
        );
        assert_eq!(
            clause_active_token_offset("json_data.contact eq", false),
            "json_data.contact ".len()
        );
    }

    #[test]
    fn where_status_suggestions_render_visible_menu_entries() {
        let suggestion = where_suggestion(
            "json_data.".to_string(),
            42,
            52,
            Some("no schema".to_string()),
            false,
        );

        assert_eq!(suggestion.value, "json_data.");
        assert_eq!(suggestion.display_override.as_deref(), Some("no schema"));
        assert!(!suggestion.append_whitespace);

        let fallback = where_suggestion(
            "json_data.".to_string(),
            42,
            52,
            Some("type path manually".to_string()),
            false,
        );
        assert_eq!(
            fallback.display_override.as_deref(),
            Some("type path manually")
        );
    }

    #[test]
    fn pipe_completion_context_completes_fields_after_projection_stage_without_space() {
        let line = "object list --class Hosts | P";
        let context = pipe_completion_context(line, line.len()).expect("projection context");

        assert_eq!(context.command_prefix, "object list --class Hosts ");
        assert_eq!(context.kind, PipeCompletionKind::Field);
        assert_eq!(context.prefix, "");
        assert_eq!(context.replacement_start, line.len());
        assert!(context.needs_leading_space);
    }

    #[test]
    fn pipe_completion_context_replaces_only_projection_comma_segment() {
        let line = "object list --class Hosts | P Name,co";
        let context = pipe_completion_context(line, line.len()).expect("projection context");

        assert_eq!(context.kind, PipeCompletionKind::Field);
        assert_eq!(context.prefix, "co");
        assert_eq!(context.replacement_start, line.len() - "co".len());
        assert!(!context.needs_leading_space);
    }

    #[test]
    fn pipe_completion_context_completes_stage_after_pipe() {
        let line = "object list --class Hosts |";
        let context = pipe_completion_context(line, line.len()).expect("stage context");

        assert_eq!(context.kind, PipeCompletionKind::Stage);
        assert_eq!(context.prefix, "");
        assert_eq!(context.replacement_start, line.len());
        assert!(context.needs_leading_space);
    }

    #[test]
    fn pipe_completion_context_completes_value_stage_fields() {
        let line = "object list --class Hosts | VALUE da";
        let context = pipe_completion_context(line, line.len()).expect("field context");

        assert_eq!(context.kind, PipeCompletionKind::Field);
        assert_eq!(context.prefix, "da");
        assert_eq!(context.replacement_start, line.len() - "da".len());
    }

    #[test]
    fn clause_option_context_accepts_inline_where_values() {
        let parts = vec![
            "namespace".to_string(),
            "list".to_string(),
            "--where=na".to_string(),
        ];
        let context = clause_option_context(
            &parts,
            "--where",
            3,
            "namespace list ".len(),
            "namespace list --where=na".len(),
            "--where=na",
            false,
        )
        .expect("where context");

        assert_eq!(context.clause_prefix, "na");
        assert_eq!(context.prefix, "na");
        assert_eq!(context.replacement_start, "namespace list --where=".len());
        assert!(!context.is_complete);
    }

    #[test]
    fn projection_schema_paths_include_nested_data_fields() {
        let schema = serde_json::json!({
            "properties": {
                "contact": {"type": "string"},
                "hardware": {
                    "type": "object",
                    "properties": {
                        "cpu": {"type": "string"}
                    }
                },
                "interfaces": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "ipv4": {"type": "string"}
                        }
                    }
                }
            }
        });

        assert_eq!(
            schema_paths(&schema, true),
            vec![
                "contact".to_string(),
                "hardware".to_string(),
                "hardware.cpu".to_string(),
                "interfaces".to_string(),
                "interfaces[*].ipv4".to_string()
            ]
        );
    }

    #[test]
    fn id_completion_context_detects_task_option_value() {
        let parts = vec![
            "task".to_string(),
            "show".to_string(),
            "--id".to_string(),
            "12".to_string(),
        ];
        let context = id_completion_context(
            &["task".to_string(), "show".to_string()],
            &parts,
            "task show --id ".len(),
            "task show --id 12".len(),
            "12",
            false,
        )
        .expect("task id context");

        assert_eq!(context.kind, IdCompletionKind::Task);
        assert_eq!(context.prefix, "12");
        assert_eq!(context.replacement_start, "task show --id ".len());
    }

    #[test]
    fn id_completion_context_detects_positional_local_job_id() {
        let parts = vec!["jobs".to_string(), "show".to_string()];
        let context = id_completion_context(
            &["jobs".to_string(), "show".to_string()],
            &parts,
            "jobs show".len(),
            "jobs show ".len(),
            "",
            true,
        )
        .expect("local job id context");

        assert_eq!(context.kind, IdCompletionKind::LocalJob);
        assert_eq!(context.prefix, "");
        assert_eq!(context.replacement_start, "jobs show ".len());
    }

    #[test]
    fn id_completion_context_detects_import_task_id() {
        let parts = vec!["import".to_string(), "results".to_string(), "7".to_string()];
        let context = id_completion_context(
            &["import".to_string(), "results".to_string()],
            &parts,
            "import results ".len(),
            "import results 7".len(),
            "7",
            false,
        )
        .expect("import task id context");

        assert_eq!(context.kind, IdCompletionKind::ImportTask);
        assert_eq!(context.prefix, "7");
    }

    #[test]
    fn id_completion_context_detects_inline_task_option_value() {
        let parts = vec![
            "task".to_string(),
            "show".to_string(),
            "--id=12".to_string(),
        ];
        let context = id_completion_context(
            &["task".to_string(), "show".to_string()],
            &parts,
            "task show ".len(),
            "task show --id=12".len(),
            "--id=12",
            false,
        )
        .expect("task id context");

        assert_eq!(context.kind, IdCompletionKind::Task);
        assert_eq!(context.prefix, "12");
        assert_eq!(context.replacement_start, "task show --id=".len());
    }

    fn test_option(short: Option<&str>, long: Option<&str>, flag: bool, help: &str) -> OptionSpec {
        OptionSpec {
            name: "name".to_string(),
            short: short.map(str::to_string),
            long: long.map(str::to_string),
            help: help.to_string(),
            field_type_help: "string".to_string(),
            field_type: TypeId::of::<String>(),
            required: false,
            flag,
            greedy: false,
            nargs: None,
            repeatable: false,
            value_source: false,
            completion: CompletionSpec::None,
        }
    }

    fn esc_event() -> ReedlineRawEvent {
        ReedlineRawEvent::try_from(CrosstermEvent::Key(CrosstermKeyEvent::new(
            CrosstermKeyCode::Esc,
            CrosstermKeyModifiers::NONE,
        )))
        .expect("press events should be accepted")
    }
}
