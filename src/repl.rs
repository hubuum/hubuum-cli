use std::borrow::Cow;
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

    println!("{}", app.catalog.render_scope_help(&[]));

    loop {
        let prompt = ReplPrompt {
            left: app.prompt(&session),
        };

        let signal = editor
            .read_line(&prompt)
            .map_err(|err| AppError::ReplError(err.to_string()))?;

        match signal {
            Signal::Success(line) => {
                let result = runtime.block_on(dispatch::execute_line(app.clone(), &session, &line));
                match result {
                    Ok(outcome) => {
                        let exit_repl = outcome.scope_action == ScopeAction::ExitRepl;
                        apply_outcome(&session, outcome);
                        if exit_repl {
                            break;
                        }
                    }
                    Err(err) => {
                        print!("{}", dispatch::render_error(err).render());
                    }
                }
            }
            Signal::CtrlD => break,
            Signal::CtrlC => continue,
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
    if !outcome.output.is_empty() {
        print!("{}", outcome.output.render());
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
        Some(
            complete_where_clause(
                &self.completion,
                &resolved.command_path,
                quoted.clause_prefix,
                quoted.clause_ends_with_space,
            )
            .into_iter()
            .map(|candidate| {
                suggestion_with_whitespace(
                    candidate.value,
                    quoted.start,
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

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use crate::catalog::{CompletionSpec, OptionSpec};

    use super::{
        completion_context_parts, is_completing_option_value, option_suggestion,
        quoted_where_context, safe_prefix_end,
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
            completion: CompletionSpec::None,
        }
    }
}
