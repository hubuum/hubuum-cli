use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use reedline::{
    default_emacs_keybindings, ColumnarMenu, Completer, Emacs, FileBackedHistory, KeyCode,
    KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion,
};

use crate::app::{AppRuntime, SharedSession};
use crate::autocomplete::{complete_sort_clause, complete_where_clause};
use crate::catalog::{CommandOutcome, CompletionSpec, OptionSpec, ScopeAction};
use crate::dispatch;
use crate::errors::AppError;
use crate::files::get_history_file;

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
    let edit_mode = Box::new(Emacs::new(keybindings));

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
                        apply_outcome(&session, outcome);
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
                if crate::config::get_config().repl.enter_fetches_next_page
                    && session.next_page_command().is_some()
                {
                    session.set_next_page_command(None);
                }
                continue;
            }
        }
    }

    Ok(())
}

struct BackgroundGuard {
    manager: crate::background::BackgroundManager,
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

fn apply_outcome(session: &SharedSession, outcome: CommandOutcome) {
    dispatch::apply_scope_action(session, &outcome.scope_action);
    dispatch::apply_output_state(session, &outcome.output);
    if !outcome.output.is_empty() {
        let _ = crate::output::print_rendered(&outcome.output.render());
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
        if let Some(suggestions) = self.pipe_projection_suggestions(prefix_line, pos) {
            return suggestions;
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
            let options_seen: Vec<&str> = parts
                .iter()
                .filter(|part| part.starts_with('-'))
                .map(String::as_str)
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
                ends_with_space,
            ) {
                return suggestions;
            }

            if let Some(last) = parts.last() {
                if prefix_line.ends_with(' ') && last.starts_with('-') {
                    if let Some(option) = options.iter().find(|option| {
                        option.short.as_deref() == Some(last.as_str())
                            || option.long.as_deref() == Some(last.as_str())
                    }) {
                        if let CompletionSpec::Dynamic(completion) = option.completion.clone() {
                            return completion(&self.completion, "", &parts)
                                .into_iter()
                                .map(|value| suggestion(value, pos, pos, None))
                                .collect();
                        }
                    }
                }

                if is_completing_option_value(&parts, ends_with_space) {
                    if let Some(prev) = parts.iter().rev().nth(1) {
                        if let Some(option) = options.iter().find(|option| {
                            option.short.as_deref() == Some(prev.as_str())
                                || option.long.as_deref() == Some(prev.as_str())
                        }) {
                            if let CompletionSpec::Dynamic(completion) = option.completion.clone() {
                                return completion(&self.completion, word, &parts)
                                    .into_iter()
                                    .map(|value| suggestion(value, start, pos, None))
                                    .collect();
                            }
                        }
                    }
                }

                if last.starts_with('-') || prefix_line.ends_with(' ') {
                    return options
                        .iter()
                        .filter(|option| {
                            option.repeatable
                                || (!options_seen.contains(&option.short.as_deref().unwrap_or(""))
                                    && !options_seen
                                        .contains(&option.long.as_deref().unwrap_or("")))
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
        _word: &str,
        ends_with_space: bool,
    ) -> Option<Vec<Suggestion>> {
        let where_index = parts.iter().rposition(|part| part == "--where")?;
        let clause_parts = &parts[where_index + 1..];
        if clause_parts.len() >= 3 && !ends_with_space {
            return Some(vec![suggestion_with_whitespace(
                clause_parts.last()?.clone(),
                start,
                pos,
                None,
                true,
            )]);
        }
        if clause_parts.len() >= 3 && ends_with_space {
            return None;
        }
        let (clause_prefix, clause_ends_with_space) = if clause_parts.is_empty() {
            ("".to_string(), false)
        } else {
            let mut clause = clause_parts.join(" ");
            if ends_with_space {
                clause.push(' ');
            }
            (clause, ends_with_space)
        };

        Some(
            complete_where_clause(
                &self.completion,
                command_path,
                parts,
                &clause_prefix,
                clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                where_suggestion(
                    candidate.value,
                    start,
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
        ends_with_space: bool,
    ) -> Option<Vec<Suggestion>> {
        let sort_index = parts.iter().rposition(|part| part == "--sort")?;
        let clause_parts = &parts[sort_index + 1..];
        if clause_parts.len() >= 2 && !ends_with_space {
            return Some(vec![suggestion_with_whitespace(
                clause_parts.last()?.clone(),
                start,
                pos,
                None,
                true,
            )]);
        }
        if clause_parts.len() >= 2 && ends_with_space {
            return None;
        }
        let (clause_prefix, clause_ends_with_space) = if clause_parts.is_empty() {
            ("".to_string(), false)
        } else {
            let mut clause = clause_parts.join(" ");
            if ends_with_space {
                clause.push(' ');
            }
            (clause, ends_with_space)
        };

        Some(
            complete_sort_clause(
                &self.completion,
                command_path,
                &clause_prefix,
                clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                suggestion_with_whitespace(
                    candidate.value,
                    start,
                    pos,
                    candidate.description,
                    candidate.append_whitespace,
                )
            })
            .collect(),
        )
    }

    fn pipe_projection_suggestions(
        &self,
        prefix_line: &str,
        pos: usize,
    ) -> Option<Vec<Suggestion>> {
        let context = projection_pipe_context(prefix_line, pos)?;
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

struct ProjectionPipeContext<'a> {
    command_prefix: &'a str,
    prefix: &'a str,
    replacement_start: usize,
    needs_leading_space: bool,
}

fn projection_pipe_context(prefix_line: &str, pos: usize) -> Option<ProjectionPipeContext<'_>> {
    let pipe_index = last_unquoted_pipe(prefix_line)?;
    let command_prefix = &prefix_line[..pipe_index];
    let pipe_prefix = &prefix_line[pipe_index + 1..];
    let pipe_parts = shlex::split(pipe_prefix)?;
    let stage = pipe_parts.first()?;
    if stage != "P" && stage != "columns" {
        return None;
    }

    let ends_with_space = prefix_line.ends_with(char::is_whitespace);
    if pipe_parts.len() == 1 && !ends_with_space {
        return Some(ProjectionPipeContext {
            command_prefix,
            prefix: "",
            replacement_start: pos,
            needs_leading_space: true,
        });
    }

    let (word_start, word) = if ends_with_space {
        (pos, "")
    } else {
        prefix_line
            .rsplit_once(char::is_whitespace)
            .map_or((0, prefix_line), |(left, right)| (left.len() + 1, right))
    };
    let comma_offset = word.rfind(',').map(|index| index + 1).unwrap_or(0);
    Some(ProjectionPipeContext {
        command_prefix,
        prefix: &word[comma_offset..],
        replacement_start: word_start + comma_offset,
        needs_leading_space: false,
    })
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
            for path in schema_paths(&schema) {
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

fn schema_paths(schema: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_schema_paths(schema, "", &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_schema_paths(schema: &serde_json::Value, prefix: &str, paths: &mut Vec<String>) {
    let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
        return;
    };

    for (name, property_schema) in properties {
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        paths.push(path.clone());
        collect_schema_paths(property_schema, &path, paths);
    }
}

fn completion_context_parts(parts: &[String], ends_with_space: bool) -> &[String] {
    if ends_with_space || parts.is_empty() {
        parts
    } else {
        &parts[..parts.len().saturating_sub(1)]
    }
}

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

    use crate::catalog::{CompletionSpec, OptionSpec};

    use super::{
        clause_active_token_offset, completion_context_parts, is_completing_option_value,
        option_suggestion, projection_pipe_context, quoted_where_context, safe_prefix_end,
        schema_paths, where_suggestion,
    };

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
    fn projection_pipe_context_completes_after_stage_without_space() {
        let line = "object list --class Hosts | P";
        let context = projection_pipe_context(line, line.len()).expect("projection context");

        assert_eq!(context.command_prefix, "object list --class Hosts ");
        assert_eq!(context.prefix, "");
        assert_eq!(context.replacement_start, line.len());
        assert!(context.needs_leading_space);
    }

    #[test]
    fn projection_pipe_context_replaces_only_comma_segment() {
        let line = "object list --class Hosts | P Name,co";
        let context = projection_pipe_context(line, line.len()).expect("projection context");

        assert_eq!(context.prefix, "co");
        assert_eq!(context.replacement_start, line.len() - "co".len());
        assert!(!context.needs_leading_space);
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
                }
            }
        });

        assert_eq!(
            schema_paths(&schema),
            vec![
                "contact".to_string(),
                "hardware".to_string(),
                "hardware.cpu".to_string()
            ]
        );
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
}
