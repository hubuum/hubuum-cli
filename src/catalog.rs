use std::any::TypeId;
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use hubuum_filter::PipeStage;

use crate::errors::AppError;
use crate::list_query::{completion_operators, FilterOperatorProfile};
use crate::output::OutputSnapshot;
use crate::services::filter_specs_for_command_path;
use crate::terminal::terminal_width;
use crate::theme::{paint, paint_command, ThemeRole};

#[derive(Debug, Clone)]
pub struct OptionSpec {
    pub name: String,
    pub short: Option<String>,
    pub long: Option<String>,
    pub help: String,
    pub field_type_help: String,
    pub field_type: TypeId,
    pub required: bool,
    pub flag: bool,
    pub greedy: bool,
    pub nargs: Option<usize>,
    pub repeatable: bool,
    pub value_source: bool,
    pub completion: CompletionSpec,
}

#[derive(Debug, Clone)]
pub enum CompletionSpec {
    None,
    Dynamic(crate::commands::AutoCompleter),
}

#[derive(Debug, Clone, Default)]
pub struct ScopeSpec {
    pub name: String,
    pub commands: BTreeMap<String, CommandSpec>,
    pub scopes: BTreeMap<String, ScopeSpec>,
}

#[derive(Clone)]
pub struct CommandSpec {
    pub name: String,
    pub about: Option<String>,
    pub long_about: Option<String>,
    pub examples: Option<String>,
    pub options: Vec<OptionSpec>,
    pub handler: Arc<dyn AsyncCommandHandler>,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec")
            .field("name", &self.name)
            .field("about", &self.about)
            .field("long_about", &self.long_about)
            .field("examples", &self.examples)
            .field("options", &self.options)
            .finish()
    }
}

#[async_trait]
pub trait AsyncCommandHandler: Send + Sync {
    async fn execute(
        &self,
        ctx: CommandContext,
        invocation: CommandInvocation,
    ) -> Result<CommandOutcome, AppError>;
}

#[derive(Clone)]
pub struct CommandCatalog {
    root: ScopeSpec,
}

#[derive(Clone)]
pub struct CommandContext {
    pub app: Arc<crate::app::AppRuntime>,
}

#[derive(Debug, Clone)]
pub struct CommandInvocation {
    pub raw_line: String,
    pub command_path: Vec<String>,
    pub pipeline: Vec<PipeStage>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandOutcome {
    pub output: OutputSnapshot,
    pub redirect: Option<crate::redirection::OutputRedirect>,
    pub scope_action: ScopeAction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ScopeAction {
    #[default]
    None,
    Enter(Vec<String>),
    ExitScope,
    ExitRepl,
}

#[derive(Default)]
pub struct CommandCatalogBuilder {
    root: ScopeSpec,
}

impl ScopeSpec {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            commands: BTreeMap::new(),
            scopes: BTreeMap::new(),
        }
    }
}

impl CommandCatalogBuilder {
    pub fn new() -> Self {
        Self {
            root: ScopeSpec::new("root"),
        }
    }

    pub fn add_command(&mut self, path: &[&str], command: CommandSpec) -> &mut Self {
        let mut current = &mut self.root;
        for segment in path {
            current = current
                .scopes
                .entry((*segment).to_string())
                .or_insert_with(|| ScopeSpec::new(*segment));
        }
        current.commands.insert(command.name.clone(), command);
        self
    }

    pub fn build(self) -> CommandCatalog {
        CommandCatalog { root: self.root }
    }
}

impl CommandCatalog {
    pub fn scope(&self, path: &[String]) -> Option<&ScopeSpec> {
        let mut current = &self.root;
        for segment in path {
            current = current.scopes.get(segment)?;
        }
        Some(current)
    }

    pub fn resolve_command<'a>(
        &'a self,
        scope: &[String],
        parts: &[String],
    ) -> Result<ResolvedCommand<'a>, AppError> {
        if parts.is_empty() {
            return Err(AppError::CommandNotFound("No command".to_string()));
        }

        let current_scope = self
            .scope(scope)
            .ok_or_else(|| AppError::CommandNotFound(scope.join(" ")))?;

        let mut effective_scope = scope.to_vec();
        let mut traversed = current_scope;

        for part in parts {
            if let Some(next_scope) = traversed.scopes.get(part) {
                effective_scope.push(part.clone());
                traversed = next_scope;
                continue;
            }

            if let Some(command) = traversed.commands.get(part) {
                let mut command_path = effective_scope.clone();
                command_path.push(part.clone());
                return Ok(ResolvedCommand {
                    scope_path: effective_scope,
                    command_path,
                    command,
                });
            }

            let message = command_not_found_message(part, traversed);
            return Err(AppError::CommandNotFound(message));
        }

        Err(AppError::CommandNotFound(parts.join(" ")))
    }

    pub fn resolve_scope<'a>(
        &'a self,
        scope: &[String],
        parts: &[String],
    ) -> Option<&'a ScopeSpec> {
        let mut current = self.scope(scope)?;
        for part in parts {
            current = current.scopes.get(part)?;
        }
        Some(current)
    }

    pub fn list_words(&self, scope: &[String]) -> Vec<String> {
        let Some(scope_spec) = self.scope(scope) else {
            return Vec::new();
        };

        scope_spec
            .scopes
            .keys()
            .chain(scope_spec.commands.keys())
            .cloned()
            .collect()
    }

    pub fn render_scope_help(&self, scope: &[String]) -> String {
        let Some(scope_spec) = self.scope(scope) else {
            return String::new();
        };

        let mut lines = Vec::new();
        let title = if scope.is_empty() {
            format!("Available commands ({})", scope_spec.name)
        } else {
            format!("Scope: {}", scope.join(" "))
        };
        lines.push(paint(ThemeRole::Heading, title));

        if !scope_spec.scopes.is_empty() {
            lines.push(String::new());
            lines.push(paint(ThemeRole::Heading, "Scopes:"));
            let name_width = scope_spec
                .scopes
                .keys()
                .map(String::len)
                .max()
                .unwrap_or(0)
                .max(16);
            for (scope_name, nested_scope) in &scope_spec.scopes {
                let summary = scope_command_summary(nested_scope);
                if summary.is_empty() {
                    lines.push(format!("  {scope_name}"));
                } else {
                    lines.extend(render_scope_summary(scope_name, &summary, name_width));
                }
            }
        }

        if !scope_spec.commands.is_empty() {
            lines.push(String::new());
            lines.push(paint(ThemeRole::Heading, "Commands:"));
            for command in scope_spec.commands.values() {
                let about = command.about.clone().unwrap_or_default();
                if about.is_empty() {
                    lines.push(format!("  {}", command.name));
                } else {
                    lines.push(format!("  {:<16} {}", command.name, about));
                }
            }
        }

        lines.push(String::new());
        lines.extend(render_pipe_help_lines());

        lines.push(String::new());
        lines.extend(render_shell_help_lines());

        lines.join("\n")
    }

    pub fn render_tree(&self) -> String {
        let mut lines = Vec::new();
        render_tree_scope(&self.root, String::new(), &mut lines);
        lines.join("\n")
    }

    pub fn render_command_help(&self, command_path: &[String]) -> Result<String, AppError> {
        if command_path.is_empty() {
            return Err(AppError::CommandNotFound("".to_string()));
        }
        let scope = &command_path[..command_path.len() - 1];
        let name = &command_path[command_path.len() - 1];
        let scope_spec = self
            .scope(scope)
            .ok_or_else(|| AppError::CommandNotFound(scope.join(" ")))?;
        let command = scope_spec
            .commands
            .get(name)
            .ok_or_else(|| AppError::CommandNotFound(name.clone()))?;

        let mut help = String::new();
        help.push_str(&paint(ThemeRole::Heading, command_path.join(" ")));
        if let Some(about) = &command.about {
            help.push_str(" - ");
            help.push_str(about);
        }
        help.push_str("\n\n");

        if let Some(long_about) = &command.long_about {
            help.push_str(long_about);
            help.push_str("\n\n");
        }

        if !command.options.is_empty() {
            help.push_str(&paint(ThemeRole::Heading, "Options:"));
            help.push('\n');
            for option in &command.options {
                let mut names = Vec::new();
                if let Some(short) = &option.short {
                    names.push(short.clone());
                }
                if let Some(long) = &option.long {
                    names.push(long.clone());
                }
                let label = if names.is_empty() {
                    option.name.clone()
                } else {
                    format!("{} ({})", names.join(", "), option.name)
                };
                let field_type = if option.flag {
                    "(flag)".to_string()
                } else {
                    format!("<{}>", option.field_type_help)
                };
                let mut annotations = Vec::new();
                if option.required {
                    annotations.push("required");
                }
                if option.repeatable {
                    annotations.push("repeatable");
                }
                let nargs_annotation;
                if let Some(nargs) = option.nargs {
                    nargs_annotation = format!("nargs={nargs}");
                    annotations.push(&nargs_annotation);
                }
                if option.value_source {
                    annotations.push("value-source");
                }
                let annotations = if annotations.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", annotations.join(", "))
                };
                help.push_str(&format!(
                    "  {:<28} {:<16} {}{}\n",
                    label, field_type, option.help, annotations
                ));
            }
            help.push('\n');
        }

        if let Some(where_help) = render_where_help(command_path) {
            help.push_str(&where_help);
            help.push('\n');
        }

        if let Some(pagination_help) = render_pagination_help(command) {
            help.push_str(&pagination_help);
            help.push('\n');
        }

        help.push_str(&render_pipe_help());
        help.push('\n');

        if let Some(examples) = &command.examples {
            help.push_str(&paint(ThemeRole::Heading, "Examples:"));
            help.push('\n');
            for line in examples.lines() {
                help.push_str(&paint_command(format!(
                    "  {} {}",
                    command_path.join(" "),
                    line
                )));
                help.push('\n');
            }
        }

        Ok(help.trim_end().to_string())
    }

    pub fn render_pipe_topic_help(&self, topic: Option<&str>) -> Result<String, AppError> {
        render_pipe_topic_help(topic)
    }

    pub fn render_shell_topic_help(&self, topic: Option<&str>) -> Result<String, AppError> {
        render_shell_topic_help(topic)
    }
}

fn command_not_found_message(part: &str, scope: &ScopeSpec) -> String {
    let candidates = scope
        .scopes
        .keys()
        .chain(scope.commands.keys())
        .cloned()
        .collect::<Vec<_>>();
    match crate::suggestions::did_you_mean_message(part, candidates) {
        Some(hint) => format!("{part}. {hint}"),
        None => part.to_string(),
    }
}

fn scope_command_summary(scope: &ScopeSpec) -> String {
    scope
        .scopes
        .keys()
        .chain(scope.commands.keys())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_scope_summary(scope_name: &str, summary: &str, name_width: usize) -> Vec<String> {
    let terminal_width = terminal_width().unwrap_or(120);
    render_scope_summary_at_width(scope_name, summary, name_width, terminal_width)
}

fn render_scope_summary_at_width(
    scope_name: &str,
    summary: &str,
    name_width: usize,
    terminal_width: usize,
) -> Vec<String> {
    let inline_width = 2 + name_width + 1 + summary.len();

    if inline_width <= terminal_width {
        return vec![format!("  {scope_name:<name_width$} {summary}")];
    }

    let summary_width = terminal_width.saturating_sub(2 + name_width + 1).max(24);
    let mut wrapped = wrap_comma_list(summary, summary_width).into_iter();
    let mut lines = Vec::new();
    if let Some(first) = wrapped.next() {
        lines.push(format!("  {scope_name:<name_width$} {first}"));
    }
    lines.extend(wrapped.map(|line| format!("  {:<name_width$} {line}", "")));
    lines
}

fn wrap_comma_list(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for item in text.split(", ") {
        let candidate = if current.is_empty() {
            item.to_string()
        } else {
            format!("{current}, {item}")
        };
        if !current.is_empty() && candidate.len() > width {
            lines.push(current);
            current = item.to_string();
        } else {
            current = candidate;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn render_pipe_help() -> String {
    render_pipe_help_lines().join("\n") + "\n"
}

fn render_pipe_help_lines() -> Vec<String> {
    vec![
        paint(ThemeRole::Heading, "Pipe:"),
        format!(
            "  Use {} for output pipeline syntax, filters, projections, sorting, and examples.",
            paint_command("help pipe")
        ),
    ]
}

fn render_shell_help_lines() -> Vec<String> {
    vec![
        paint(ThemeRole::Heading, "Shell:"),
        format!(
            "  Use {} for REPL navigation, pagination, and exit commands.",
            paint_command("help shell")
        ),
    ]
}

fn render_pipe_topic_help(topic: Option<&str>) -> Result<String, AppError> {
    let mut lines = Vec::new();
    macro_rules! line {
        ($value:expr) => {
            lines.push($value.to_string());
        };
    }

    if let Some(topic) = topic {
        let Some(help) = hubuum_filter::topic_help(topic) else {
            return Err(AppError::CommandNotFound(format!("pipe {topic}")));
        };
        line!(paint(ThemeRole::Heading, format!("Pipe: {topic}")));
        line!(colorize_help_commands(help));
        return Ok(lines.join("\n"));
    }

    line!(paint(ThemeRole::Heading, "Pipe"));
    line!("Append pipe stages after a command to transform semantic output before table, JSON, JSONL, CSV, TSV, or text rendering.");
    line!("");
    line!(paint(ThemeRole::Heading, "Topics:"));
    for topic in hubuum_filter::help_topics() {
        line!(format!(
            "  {} {}",
            paint_command(format!("help pipe {:<10}", topic.name)),
            topic.summary
        ));
    }
    line!("");
    line!(paint(ThemeRole::Heading, "Verbs:"));
    for summary in hubuum_filter::verb_summaries() {
        line!(format!(
            "  {:<14} {:<9} {}",
            summary.names, summary.topic, summary.summary
        ));
    }
    line!("");
    line!("Examples:");
    line!(paint_command(
        "  object list --class Hosts | grep <field> <regex>"
    ));
    line!(paint_command(
        "  object list --class Hosts | P os_version 26"
    ));
    line!(paint_command(
        "  object list --class Hosts | F os_version contains 26"
    ));
    line!(paint_command(
        "  object list --class Hosts | P Name os_version | S os_version | L 10"
    ));
    line!(paint_command(
        "  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts"
    ));

    Ok(lines.join("\n"))
}

fn render_shell_topic_help(topic: Option<&str>) -> Result<String, AppError> {
    let mut lines = Vec::new();
    macro_rules! line {
        ($value:expr) => {
            lines.push($value.to_string());
        };
    }

    if let Some(topic) = topic {
        line!(paint(ThemeRole::Heading, format!("Shell: {topic}")));
        match topic {
            "navigation" => {
                line!(format!(
                    "  Type a scope name to enter it, for example {} or {}.",
                    paint_command("object"),
                    paint_command("namespace")
                ));
                line!("  Type a nested scope name to descend further.");
                line!(format!(
                    "  Use {} to leave the current scope.",
                    paint_command("..")
                ));
                line!(format!(
                    "  Use {} to leave the current scope, or {} at root to leave the REPL.",
                    paint_command("exit"),
                    paint_command("exit")
                ));
                line!("  Use Ctrl-D to leave the REPL.");
            }
            "pagination" => {
                line!(format!(
                    "  After a paginated list result, type {} to fetch the next page.",
                    paint_command("next")
                ));
                line!("  If repl.enter_fetches_next_page is enabled, pressing Enter fetches the next page.");
                line!("  Esc or Ctrl-C clears pending pagination state.");
            }
            "completion" => {
                line!("  Press Tab to open or advance completions.");
                line!("  Press Shift-Tab to move backward in the completion menu.");
                line!(format!(
                    "  Option values complete after either {} or {}.",
                    paint_command("--option <value>"),
                    paint_command("--option=<value>")
                ));
                line!("  Pipe stages and supported field names complete after |.");
                line!(format!(
                    "  API-backed completions can be disabled with {}.",
                    paint_command("--completion-api-disable true")
                ));
            }
            "redirects" => {
                line!(format!(
                    "  Append {} to write rendered output, or {} to append.",
                    paint_command("> <file>"),
                    paint_command(">> <file>")
                ));
                line!(format!(
                    "  Use {} to write one file per semantic row or value.",
                    paint_command("> each:<template>")
                ));
                line!(
                    "  each: templates accept field placeholders such as {Name}, {value}, and {n}."
                );
                line!("  Redirect paths complete like normal file path arguments.");
            }
            _ => return Err(AppError::CommandNotFound(format!("shell {topic}"))),
        }
        return Ok(lines.join("\n"));
    }

    line!(paint(ThemeRole::Heading, "Shell"));
    line!("The REPL shell keeps a current scope, command history, and next-page state.");
    line!("");
    line!(paint(ThemeRole::Heading, "Help:"));
    line!(paint_command("  help"));
    line!(paint_command("  help <scope>"));
    line!(paint_command("  help <command>"));
    line!(paint_command("  ? <command>"));
    line!(paint_command("  help pipe"));
    line!(paint_command("  help pipe <topic>"));
    line!(paint_command("  help shell"));
    line!(paint_command("  help shell <topic>"));
    line!("");
    line!(paint(ThemeRole::Heading, "Topics:"));
    line!(format!(
        "  {} Type a scope name to enter it.",
        paint_command("help shell navigation")
    ));
    line!(format!(
        "  {} Type next to fetch the next page.",
        paint_command("help shell pagination")
    ));
    line!(format!("  {}", paint_command("help shell completion")));
    line!(format!("  {}", paint_command("help shell redirects")));
    line!("");
    line!("Pipes:");
    line!(format!(
        "  Append {} stages after commands to filter or reshape output before rendering.",
        paint_command("|")
    ));
    line!(format!(
        "  Use {} for pipeline syntax and examples.",
        paint_command("help pipe")
    ));

    Ok(lines.join("\n"))
}

fn colorize_help_commands(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if is_command_help_line(trimmed) {
                let indent_len = line.len() - trimmed.len();
                if let Some((command, description)) = trimmed.split_once(" - ") {
                    format!(
                        "{}{} - {}",
                        &line[..indent_len],
                        paint_command(command),
                        description
                    )
                } else {
                    format!("{}{}", &line[..indent_len], paint_command(trimmed))
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_command_help_line(trimmed: &str) -> bool {
    trimmed.starts_with('|')
        || trimmed.starts_with('>')
        || trimmed.starts_with(">>")
        || trimmed.starts_with("object ")
        || trimmed.starts_with("help ")
        || trimmed.starts_with("? ")
}

fn render_where_help(command_path: &[String]) -> Option<String> {
    let specs = filter_specs_for_command_path(command_path)?;
    let fields = specs
        .iter()
        .map(|spec| {
            if spec.json_root {
                format!("{}.<path>", spec.public_name)
            } else {
                spec.public_name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut operator_profiles = specs
        .iter()
        .map(|spec| spec.operator_profile)
        .collect::<Vec<_>>();
    operator_profiles.sort_by_key(operator_profile_rank);
    operator_profiles.dedup();

    let operators = operator_profiles
        .into_iter()
        .flat_map(completion_operators)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");

    let mut help = String::new();
    help.push_str(&paint(ThemeRole::Heading, "Where:"));
    help.push('\n');
    help.push_str(&format!(
        "  Syntax: {}",
        paint_command("--where 'field operator value'")
    ));
    help.push('\n');
    help.push_str(&format!(
        "  Repeat {} to combine filters with AND.\n",
        paint_command("--where")
    ));
    help.push_str("  Fields: ");
    help.push_str(&fields);
    help.push('\n');
    help.push_str("  Operators: ");
    help.push_str(&operators);
    help.push('\n');
    help.push_str(
        "  Negation: prefix an operator with not_, such as not_equals or not_icontains.\n",
    );

    if specs.iter().any(|spec| spec.json_root) {
        help.push_str(&format!(
            "  JSON paths: use dotted paths, for example {}.\n",
            paint_command("json_data.contact equals Entry")
        ));
    }

    Some(help)
}

fn operator_profile_rank(profile: &FilterOperatorProfile) -> u8 {
    match profile {
        FilterOperatorProfile::EqualityOnly => 0,
        FilterOperatorProfile::Boolean => 1,
        FilterOperatorProfile::String => 2,
        FilterOperatorProfile::NumericOrDate => 3,
        FilterOperatorProfile::Any => 4,
    }
}

fn render_pagination_help(command: &CommandSpec) -> Option<String> {
    let option_names = command
        .options
        .iter()
        .map(|option| option.name.as_str())
        .collect::<Vec<_>>();

    if !option_names.contains(&"limit") && !option_names.contains(&"cursor") {
        return None;
    }

    let mut help = String::new();
    help.push_str(&paint(ThemeRole::Heading, "Result Limits:"));
    help.push('\n');
    if option_names.contains(&"limit") {
        help.push_str(&format!(
            "  Use {} to cap the number of returned results.",
            paint_command("--limit <n>")
        ));
        help.push('\n');
    }
    if option_names.contains(&"cursor") {
        help.push_str(&format!(
            "  Use {} to continue from a previous page.",
            paint_command("--cursor <token>")
        ));
        help.push('\n');
        help.push_str(&format!(
            "  In the REPL, type {} after a result with a next-page cursor to reuse it.\n",
            paint_command("next")
        ));
        help.push_str(&paint(
            ThemeRole::Muted,
            "  Esc or Ctrl-C clears the pending pagination state.\n",
        ));
        help.push_str(&paint(
            ThemeRole::Muted,
            "  If repl.enter_fetches_next_page is enabled, pressing Enter fetches the next page.\n",
        ));
    }
    Some(help)
}

impl OptionSpec {
    pub fn to_cli_option(&self) -> crate::commands::CliOption {
        crate::commands::CliOption {
            name: self.name.clone(),
            short: self.short.clone(),
            long: self.long.clone(),
            flag: self.flag,
            greedy: self.greedy,
            nargs: self.nargs,
            repeatable: self.repeatable,
            value_source: self.value_source,
            help: self.help.clone(),
            field_type: self.field_type,
            field_type_help: self.field_type_help.clone(),
            required: self.required,
            autocomplete: match self.completion {
                CompletionSpec::None => None,
                CompletionSpec::Dynamic(function) => Some(function),
            },
        }
    }
}

fn render_tree_scope(scope: &ScopeSpec, prefix: String, lines: &mut Vec<String>) {
    for command in scope.commands.keys() {
        lines.push(format!("{prefix}{command}"));
    }

    for (name, nested) in &scope.scopes {
        lines.push(format!("{prefix}{name}"));
        render_tree_scope(nested, format!("{prefix}{name} "), lines);
    }
}

pub struct ResolvedCommand<'a> {
    pub scope_path: Vec<String>,
    pub command_path: Vec<String>,
    pub command: &'a CommandSpec,
}

#[cfg(test)]
mod tests {
    use super::{CommandCatalogBuilder, CommandSpec, ScopeAction};
    use async_trait::async_trait;
    use serial_test::serial;
    use std::sync::Arc;

    struct NoopHandler;

    #[async_trait]
    impl super::AsyncCommandHandler for NoopHandler {
        async fn execute(
            &self,
            _ctx: super::CommandContext,
            _invocation: super::CommandInvocation,
        ) -> Result<super::CommandOutcome, crate::errors::AppError> {
            Ok(super::CommandOutcome {
                output: Default::default(),
                scope_action: ScopeAction::None,
                ..Default::default()
            })
        }
    }

    fn command(name: &str) -> CommandSpec {
        CommandSpec {
            name: name.to_string(),
            about: Some("about".to_string()),
            long_about: None,
            examples: None,
            options: Vec::new(),
            handler: Arc::new(NoopHandler),
        }
    }

    fn strip_ansi(text: &str) -> String {
        regex::Regex::new(r"\x1b\[[0-9;]*m")
            .expect("ANSI regex should compile")
            .replace_all(text, "")
            .into_owned()
    }

    #[test]
    fn resolves_scope_and_command_from_nested_tree() {
        let mut builder = CommandCatalogBuilder::new();
        builder.add_command(&["class"], command("list"));
        let catalog = builder.build();

        let resolved = catalog
            .resolve_command(&[], &["class".to_string(), "list".to_string()])
            .expect("command should resolve");
        assert_eq!(
            resolved.command_path,
            vec!["class".to_string(), "list".to_string()]
        );
        assert!(catalog.resolve_scope(&[], &["class".to_string()]).is_some());
    }

    #[test]
    fn render_command_help_includes_option_metadata() {
        let mut builder = CommandCatalogBuilder::new();
        let mut spec = command("list");
        spec.options.push(super::OptionSpec {
            name: "name".to_string(),
            short: Some("-n".to_string()),
            long: Some("--name".to_string()),
            help: "Name filter".to_string(),
            field_type_help: "string".to_string(),
            field_type: std::any::TypeId::of::<String>(),
            required: true,
            flag: false,
            greedy: false,
            nargs: None,
            repeatable: false,
            value_source: false,
            completion: super::CompletionSpec::None,
        });
        spec.options.push(super::OptionSpec {
            name: "where".to_string(),
            short: None,
            long: Some("--where".to_string()),
            help: "Filter clause".to_string(),
            field_type_help: "string".to_string(),
            field_type: std::any::TypeId::of::<String>(),
            required: false,
            flag: false,
            greedy: false,
            nargs: Some(3),
            repeatable: true,
            value_source: false,
            completion: super::CompletionSpec::None,
        });
        builder.add_command(&["class"], spec);
        let catalog = builder.build();

        let help = catalog
            .render_command_help(&["class".to_string(), "list".to_string()])
            .expect("help should render");
        assert!(help.contains("--name"));
        assert!(help.contains("[required]"));
        assert!(help.contains("Name filter"));
        assert!(help.contains("--where"));
        assert!(help.contains("[repeatable, nargs=3]"));
    }

    #[test]
    #[serial]
    fn render_command_help_colors_example_commands_when_enabled() {
        let previous = crate::config::get_config();
        let mut config = previous.clone();
        config.output.color = crate::models::OutputColor::Always;
        crate::config::init_config(config).expect("config update");

        let mut builder = CommandCatalogBuilder::new();
        let mut spec = command("list");
        spec.examples = Some("--class Hosts".to_string());
        builder.add_command(&["object"], spec);
        let catalog = builder.build();

        let help = catalog
            .render_command_help(&["object".to_string(), "list".to_string()])
            .expect("help should render");

        crate::config::init_config(previous).expect("config restore");
        assert!(help.contains("\u{1b}["));
        assert!(help.contains("object list --class Hosts"));
    }

    #[test]
    #[serial]
    fn render_command_help_omits_command_color_when_disabled() {
        let previous = crate::config::get_config();
        let mut config = previous.clone();
        config.output.color = crate::models::OutputColor::Never;
        crate::config::init_config(config).expect("config update");

        let mut builder = CommandCatalogBuilder::new();
        let mut spec = command("list");
        spec.examples = Some("--class Hosts".to_string());
        builder.add_command(&["object"], spec);
        let catalog = builder.build();

        let help = catalog
            .render_command_help(&["object".to_string(), "list".to_string()])
            .expect("help should render");

        crate::config::init_config(previous).expect("config restore");
        assert!(!help.contains("\u{1b}["));
        assert!(help.contains("object list --class Hosts"));
    }

    #[test]
    fn option_spec_round_trips_nargs_to_cli_option() {
        let option = super::OptionSpec {
            name: "where_clauses".to_string(),
            short: None,
            long: Some("--where".to_string()),
            help: "Filter clause".to_string(),
            field_type_help: "string".to_string(),
            field_type: std::any::TypeId::of::<Vec<String>>(),
            required: false,
            flag: false,
            greedy: false,
            nargs: Some(3),
            repeatable: true,
            value_source: false,
            completion: super::CompletionSpec::None,
        };

        assert_eq!(option.to_cli_option().nargs, Some(3));
    }

    #[test]
    fn root_scope_help_lists_scope_subcommands() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog.render_scope_help(&[]);
        let plain = strip_ansi(&help);

        assert!(plain.contains("class"));
        assert!(plain.contains("create, delete, list, modify, show"));
        assert!(plain.contains("object"));
        assert!(plain.contains("create, delete, list, modify, show"));
        assert!(plain.contains("event-subscription create, delete, list, show, update"));
        assert!(plain.contains("namespace          permissions, create, delete, list, modify"));
        assert!(plain.contains("                   principal-permissions, show"));
        assert!(plain.contains("relation"));
        assert!(plain.contains("class, object"));
        assert!(plain.contains("Pipe:"));
        assert!(plain.contains("Use help pipe"));
        assert!(!plain.contains("Examples: object list --class Hosts"));
        assert!(plain.contains("Shell:"));
        assert!(plain.contains("Use help shell"));
        assert!(!plain.contains("repl.enter_fetches_next_page"));
    }

    #[test]
    #[serial]
    fn scope_help_colors_only_command_fragment_in_prose() {
        let previous = crate::config::get_config();
        let mut config = previous.clone();
        config.output.color = crate::models::OutputColor::Always;
        crate::config::init_config(config).expect("config update");

        let catalog = crate::commands::build_command_catalog();
        let help = catalog.render_scope_help(&[]);
        let line = help
            .lines()
            .find(|line| strip_ansi(line).contains("Use help pipe"))
            .expect("pipe help line should render");
        let expected = format!("Use {}", crate::theme::paint_command("help pipe"));

        crate::config::init_config(previous).expect("config restore");
        assert!(line.contains(&expected));
        assert!(!line.trim_start().starts_with("\u{1b}["));
    }

    #[test]
    fn audit_help_exposes_working_commands_and_hides_unsupported_filters() {
        let catalog = crate::commands::build_command_catalog();
        let scope_help = catalog.render_scope_help(&["audit".to_string()]);
        let list_help = catalog
            .render_command_help(&["audit".to_string(), "list".to_string()])
            .expect("audit list help should render");
        let show_help = catalog
            .render_command_help(&["audit".to_string(), "show".to_string()])
            .expect("audit show help should render");

        assert!(scope_help.contains("list"));
        assert!(scope_help.contains("show"));
        assert!(scope_help.contains("resource"));
        assert!(!list_help.contains("--entity-type"));
        assert!(!list_help.contains("--entity-id"));
        assert!(show_help.contains("Show a single audit event by id"));
        assert!(show_help.contains("--id"));
    }

    #[test]
    fn scope_summary_uses_one_line_when_terminal_is_wide_enough() {
        let lines = super::render_scope_summary_at_width(
            "namespace",
            "permissions, create, delete, list, modify, principal-permissions, show",
            "event-subscription".len(),
            120,
        );

        assert_eq!(
            lines,
            vec![
                "  namespace          permissions, create, delete, list, modify, principal-permissions, show"
            ]
        );
    }

    #[test]
    fn pipe_topic_help_explains_field_specific_filters() {
        let catalog = CommandCatalogBuilder::new().build();
        let help = catalog
            .render_pipe_topic_help(None)
            .expect("pipe index should render");
        let search = catalog
            .render_pipe_topic_help(Some("search"))
            .expect("pipe search should render");
        let group = catalog
            .render_pipe_topic_help(Some("group"))
            .expect("pipe group should render");
        let help = strip_ansi(&help);
        let search = strip_ansi(&search);
        let group = strip_ansi(&group);

        assert!(help.contains("help pipe search"));
        assert!(help.contains("help pipe group"));
        assert!(search.contains("F os_version contains 26"));
        assert!(search.contains("K ipv4"));
        assert!(group.contains("G os_version AS"));
        assert!(group.contains("A count AS Hosts"));
    }

    #[test]
    fn shell_topic_help_explains_repl_navigation_and_pagination() {
        let catalog = CommandCatalogBuilder::new().build();
        let help = catalog
            .render_shell_topic_help(None)
            .expect("shell index should render");
        let navigation = catalog
            .render_shell_topic_help(Some("navigation"))
            .expect("shell navigation should render");
        let pagination = catalog
            .render_shell_topic_help(Some("pagination"))
            .expect("shell pagination should render");
        let completion = catalog
            .render_shell_topic_help(Some("completion"))
            .expect("shell completion should render");
        let redirects = catalog
            .render_shell_topic_help(Some("redirects"))
            .expect("shell redirects should render");
        let help = strip_ansi(&help);
        let navigation = strip_ansi(&navigation);
        let pagination = strip_ansi(&pagination);
        let completion = strip_ansi(&completion);
        let redirects = strip_ansi(&redirects);

        assert!(help.contains("help shell navigation"));
        assert!(navigation.contains("Type a scope name"));
        assert!(pagination.contains("next to fetch the next page"));
        assert!(pagination.contains("repl.enter_fetches_next_page"));
        assert!(completion.contains("Press Tab"));
        assert!(completion.contains("--option=<value>"));
        assert!(redirects.contains("Append > <file>"));
        assert!(redirects.contains(">> <file>"));
    }

    #[test]
    fn nested_scope_help_lists_generated_children() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog.render_scope_help(&["relation".to_string()]);

        assert!(help.contains("class"));
        assert!(help.contains("object"));
        assert!(help.contains("create, delete, direct, graph, list, show"));
    }

    #[test]
    fn list_command_help_includes_where_guide() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog
            .render_command_help(&["object".to_string(), "list".to_string()])
            .expect("help should render");
        let plain = strip_ansi(&help);

        assert!(plain.contains("Where:"));
        assert!(plain.contains("Syntax: --where 'field operator value'"));
        assert!(plain.contains("Repeat --where to combine filters with AND."));
        assert!(plain.contains("json_data.<path>"));
        assert!(plain.contains("data.<path>"));
        assert!(plain.contains("json_data.contact equals Entry"));
        assert!(plain.contains("Result Limits:"));
        assert!(plain.contains("Use --limit <n> to cap the number of returned results."));
        assert!(plain.contains("Use --cursor <token> to continue from a previous page."));
        assert!(plain.contains("type next"));
        assert!(plain.contains("repl.enter_fetches_next_page"));
        assert!(plain.contains("Pipe:"));
        assert!(plain.contains("Use help pipe"));
        assert!(!plain.contains("VALUE/VAL <path>"));
    }

    #[test]
    fn all_registered_commands_have_about_text() {
        let catalog = crate::commands::build_command_catalog();
        let mut missing = Vec::new();
        collect_commands_missing_about(&catalog.root, &mut Vec::new(), &mut missing);

        assert!(
            missing.is_empty(),
            "commands missing about text: {}",
            missing.join(", ")
        );
    }

    #[test]
    fn list_commands_expose_generic_filter_and_paging_options() {
        let catalog = crate::commands::build_command_catalog();

        for path in [
            vec!["class", "list"],
            vec!["group", "list"],
            vec!["namespace", "list"],
            vec!["object", "list"],
            vec!["user", "list"],
            vec!["report", "list"],
            vec!["relation", "class", "list"],
            vec!["relation", "class", "direct"],
            vec!["relation", "object", "list"],
            vec!["relation", "object", "direct"],
        ] {
            let resolved = catalog
                .resolve_command(
                    &[],
                    &path
                        .into_iter()
                        .map(|part| part.to_string())
                        .collect::<Vec<_>>(),
                )
                .expect("list command should resolve");
            let option_names = resolved
                .command
                .options
                .iter()
                .map(|option| option.name.as_str())
                .collect::<Vec<_>>();
            assert!(option_names.contains(&"where_clauses"));
            assert!(option_names.contains(&"sort_clauses"));
            assert!(option_names.contains(&"limit"));
            assert!(option_names.contains(&"cursor"));
        }

        for path in [["task", "events"], ["import", "results"]] {
            let resolved = catalog
                .resolve_command(
                    &[],
                    &path.iter().map(|part| part.to_string()).collect::<Vec<_>>(),
                )
                .expect("cursor command should resolve");
            let option_names = resolved
                .command
                .options
                .iter()
                .map(|option| option.name.as_str())
                .collect::<Vec<_>>();
            assert!(!option_names.contains(&"where_clauses"));
            assert!(option_names.contains(&"sort_clauses"));
            assert!(option_names.contains(&"limit"));
            assert!(option_names.contains(&"cursor"));
        }
    }

    #[test]
    fn command_catalog_only_exposes_allowed_id_options() {
        let catalog = crate::commands::build_command_catalog();
        let mut exposed = Vec::new();
        collect_exposed_id_options(&catalog.root, &mut Vec::new(), &mut exposed);

        let allowed = [
            "audit show --id",
            "bg forget --id",
            "bg output --id",
            "bg show --id",
            "bg watch --task",
            "event-delivery dead --id",
            "event-delivery retry --id",
            "event-delivery show --id",
            "import results --id",
            "import show --id",
            "jobs forget --id",
            "jobs output --id",
            "jobs show --id",
            "jobs watch --task",
            "service-account token revoke --token-id",
            "task events --id",
            "task output --id",
            "task show --id",
            "user token revoke --token-id",
        ];

        exposed.sort();
        assert_eq!(exposed, allowed);
    }

    #[test]
    fn audit_show_id_has_dynamic_completion() {
        let catalog = crate::commands::build_command_catalog();
        let audit_show = catalog
            .resolve_command(&[], &["audit".to_string(), "show".to_string()])
            .expect("audit show should resolve");
        let id = audit_show
            .command
            .options
            .iter()
            .find(|option| option.long.as_deref() == Some("--id"))
            .expect("audit show should expose --id");

        assert!(matches!(id.completion, super::CompletionSpec::Dynamic(_)));
    }

    #[test]
    fn existing_entity_reference_options_have_dynamic_completion() {
        let catalog = crate::commands::build_command_catalog();

        for (path, option_name) in [
            (&["group", "add_user"][..], "--groupname"),
            (&["group", "add_user"][..], "--username"),
            (&["group", "remove_user"][..], "--groupname"),
            (&["group", "remove_user"][..], "--username"),
            (&["group", "show"][..], "--groupname"),
            (&["group", "modify"][..], "--groupname"),
            (&["user", "show"][..], "--username"),
            (&["user", "delete"][..], "--username"),
            (&["user", "modify"][..], "--username"),
            (&["user", "set-password"][..], "--username"),
            (&["user", "token", "list"][..], "--username"),
            (&["user", "token", "create"][..], "--username"),
            (&["user", "token", "revoke"][..], "--username"),
            (&["service-account", "show"][..], "--name"),
            (&["service-account", "delete"][..], "--name"),
            (&["service-account", "disable"][..], "--name"),
            (&["service-account", "token", "list"][..], "--name"),
            (&["service-account", "token", "create"][..], "--name"),
            (&["service-account", "token", "revoke"][..], "--name"),
            (&["remote-target", "show"][..], "--name"),
            (&["remote-target", "update"][..], "--name"),
            (&["remote-target", "delete"][..], "--name"),
            (&["remote-target", "invoke"][..], "--name"),
            (&["event-delivery", "show"][..], "--id"),
            (&["event-delivery", "retry"][..], "--id"),
            (&["event-delivery", "dead"][..], "--id"),
            (&["audit", "resource"][..], "--name"),
            (&["namespace", "principal-permissions"][..], "--principal"),
        ] {
            let command_path = path.iter().map(|part| part.to_string()).collect::<Vec<_>>();
            let resolved = catalog
                .resolve_command(&[], &command_path)
                .unwrap_or_else(|_| panic!("{command_path:?} should resolve"));
            let option = resolved
                .command
                .options
                .iter()
                .find(|option| option.long.as_deref() == Some(option_name))
                .unwrap_or_else(|| panic!("{command_path:?} should expose {option_name}"));

            assert!(
                matches!(option.completion, super::CompletionSpec::Dynamic(_)),
                "{command_path:?} {option_name} should have dynamic completion"
            );
        }
    }

    #[test]
    fn history_commands_use_name_options_with_dynamic_completion() {
        let catalog = crate::commands::build_command_catalog();
        let class_history = catalog
            .resolve_command(&[], &["history".to_string(), "class".to_string()])
            .expect("history class should resolve");
        let object_history = catalog
            .resolve_command(&[], &["history".to_string(), "object".to_string()])
            .expect("history object should resolve");

        let class_option = class_history
            .command
            .options
            .iter()
            .find(|option| option.long.as_deref() == Some("--class"))
            .expect("history class should expose --class");
        assert!(matches!(
            class_option.completion,
            super::CompletionSpec::Dynamic(_)
        ));
        assert!(!class_history
            .command
            .options
            .iter()
            .any(|option| option.long.as_deref() == Some("--class-id")));

        let object_options = &object_history.command.options;
        let object_class = object_options
            .iter()
            .find(|option| option.long.as_deref() == Some("--class"))
            .expect("history object should expose --class");
        let object_name = object_options
            .iter()
            .find(|option| option.long.as_deref() == Some("--name"))
            .expect("history object should expose --name");
        assert!(matches!(
            object_class.completion,
            super::CompletionSpec::Dynamic(_)
        ));
        assert!(matches!(
            object_name.completion,
            super::CompletionSpec::Dynamic(_)
        ));
        assert!(!object_options.iter().any(|option| {
            matches!(
                option.long.as_deref(),
                Some("--class-id") | Some("--object-id")
            )
        }));
    }

    #[test]
    fn common_format_and_task_filters_have_dynamic_completion() {
        let catalog = crate::commands::build_command_catalog();
        let task_list = catalog
            .resolve_command(&[], &["task".to_string(), "list".to_string()])
            .expect("task list should resolve");

        for long in ["--output", "--kind", "--status"] {
            let option = task_list
                .command
                .options
                .iter()
                .find(|option| option.long.as_deref() == Some(long))
                .unwrap_or_else(|| panic!("{long} should be exposed"));
            assert!(matches!(
                option.completion,
                super::CompletionSpec::Dynamic(_)
            ));
        }

        let remote_create = catalog
            .resolve_command(&[], &["remote-target".to_string(), "create".to_string()])
            .expect("remote-target create should resolve");
        for long in ["--method", "--subject-types", "--auth-type"] {
            let option = remote_create
                .command
                .options
                .iter()
                .find(|option| option.long.as_deref() == Some(long))
                .unwrap_or_else(|| panic!("{long} should be exposed"));
            assert!(matches!(
                option.completion,
                super::CompletionSpec::Dynamic(_)
            ));
        }

        let remote_invoke = catalog
            .resolve_command(&[], &["remote-target".to_string(), "invoke".to_string()])
            .expect("remote-target invoke should resolve");
        let subject = remote_invoke
            .command
            .options
            .iter()
            .find(|option| option.long.as_deref() == Some("--subject"))
            .expect("--subject should be exposed");
        assert!(matches!(
            subject.completion,
            super::CompletionSpec::Dynamic(_)
        ));

        let report_create = catalog
            .resolve_command(&[], &["report".to_string(), "create".to_string()])
            .expect("report create should resolve");
        let content_type = report_create
            .command
            .options
            .iter()
            .find(|option| option.long.as_deref() == Some("--content-type"))
            .expect("--content-type should be exposed");
        assert!(matches!(
            content_type.completion,
            super::CompletionSpec::Dynamic(_)
        ));
    }

    #[test]
    fn command_resolution_suggests_nearby_words() {
        let catalog = crate::commands::build_command_catalog();
        let err = match catalog.resolve_command(&[], &["clas".to_string(), "list".to_string()]) {
            Ok(_) => panic!("mistyped command should fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("Did you mean 'class'?"));
    }

    fn collect_commands_missing_about(
        scope: &super::ScopeSpec,
        path: &mut Vec<String>,
        missing: &mut Vec<String>,
    ) {
        for command in scope.commands.values() {
            if command
                .about
                .as_deref()
                .is_none_or(|about| about.trim().is_empty())
            {
                let mut command_path = path.clone();
                command_path.push(command.name.clone());
                missing.push(command_path.join(" "));
            }
        }

        for nested_scope in scope.scopes.values() {
            path.push(nested_scope.name.clone());
            collect_commands_missing_about(nested_scope, path, missing);
            path.pop();
        }
    }

    fn collect_exposed_id_options(
        scope: &super::ScopeSpec,
        path: &mut Vec<String>,
        exposed: &mut Vec<String>,
    ) {
        for command in scope.commands.values() {
            let mut command_path = path.clone();
            command_path.push(command.name.clone());
            for option in &command.options {
                let long = option.long.as_deref().unwrap_or_default();
                if matches!(
                    long,
                    "--id"
                        | "--token-id"
                        | "--task"
                        | "--class-id"
                        | "--object-id"
                        | "--namespace-id"
                        | "--owner-group-id"
                        | "--sink-id"
                        | "--relation-id"
                        | "--principal-id"
                ) {
                    exposed.push(format!("{} {long}", command_path.join(" ")));
                }
            }
        }

        for nested_scope in scope.scopes.values() {
            path.push(nested_scope.name.clone());
            collect_exposed_id_options(nested_scope, path, exposed);
            path.pop();
        }
    }
}
