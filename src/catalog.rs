use std::any::TypeId;
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::errors::AppError;
use crate::list_query::{completion_operators, FilterOperatorProfile};
use crate::output::OutputSnapshot;
use crate::services::filter_specs_for_command_path;

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
}

#[derive(Debug, Clone, Default)]
pub struct CommandOutcome {
    pub output: OutputSnapshot,
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

            return Err(AppError::CommandNotFound(part.clone()));
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
        lines.push(title);

        if !scope_spec.scopes.is_empty() {
            lines.push(String::new());
            lines.push("Scopes:".to_string());
            for (scope_name, nested_scope) in &scope_spec.scopes {
                let summary = scope_command_summary(nested_scope);
                if summary.is_empty() {
                    lines.push(format!("  {scope_name}"));
                } else {
                    lines.push(format!("  {scope_name:<16} [{summary}]"));
                }
            }
        }

        if !scope_spec.commands.is_empty() {
            lines.push(String::new());
            lines.push("Commands:".to_string());
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
        lines.push("Shell:".to_string());
        if scope.is_empty() {
            lines.push("  Type a scope name to enter it.".to_string());
            lines.push("  Use help <command> or ? <command> for command help.".to_string());
            lines.push(
                "  After a paginated list result, use next to fetch the next page.".to_string(),
            );
            lines.push(
                "  If repl.enter_fetches_next_page is enabled, Enter fetches the next page and Ctrl-C clears it.".to_string(),
            );
            lines.push("  Use exit or Ctrl-D to leave the REPL.".to_string());
        } else {
            lines.push("  Type a nested scope name to descend.".to_string());
            lines.push("  Use .. to leave the current scope.".to_string());
            lines.push("  Use ? for quick help in the current scope.".to_string());
            lines.push(
                "  After a paginated list result, use next to fetch the next page.".to_string(),
            );
            lines.push(
                "  If repl.enter_fetches_next_page is enabled, Enter fetches the next page and Ctrl-C clears it.".to_string(),
            );
            lines.push(
                "  Use exit to leave the current scope, or Ctrl-D to leave the REPL.".to_string(),
            );
        }

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
        help.push_str(&command_path.join(" "));
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
            help.push_str("Options:\n");
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
                let required = if option.required { " [required]" } else { "" };
                help.push_str(&format!(
                    "  {:<28} {:<16} {}{}\n",
                    label, field_type, option.help, required
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

        if let Some(examples) = &command.examples {
            help.push_str("Examples:\n");
            for line in examples.lines() {
                help.push_str("  ");
                help.push_str(&command_path.join(" "));
                help.push(' ');
                help.push_str(line);
                help.push('\n');
            }
        }

        Ok(help.trim_end().to_string())
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
    help.push_str("Where:\n");
    help.push_str("  Syntax: --where 'field operator value'\n");
    help.push_str("  Repeat --where to combine filters with AND.\n");
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
        help.push_str(
            "  JSON paths: use dotted paths, for example json_data.contact equals Entry.\n",
        );
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
    help.push_str("Pagination:\n");
    if option_names.contains(&"limit") {
        help.push_str("  Use --limit <n> to control page size.\n");
    }
    if option_names.contains(&"cursor") {
        help.push_str("  Use --cursor <token> to continue from a previous page.\n");
        help.push_str("  In the REPL, type next after a paginated result to reuse the last next-page cursor.\n");
        help.push_str(
            "  If repl.enter_fetches_next_page is enabled, pressing Enter fetches the next page and Ctrl-C clears the pending pagination state.\n",
        );
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
            completion: super::CompletionSpec::None,
        };

        assert_eq!(option.to_cli_option().nargs, Some(3));
    }

    #[test]
    fn root_scope_help_lists_scope_subcommands() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog.render_scope_help(&[]);

        assert!(help.contains("class"));
        assert!(help.contains("[create, delete, list, modify, show]"));
        assert!(help.contains("object"));
        assert!(help.contains("[create, delete, list, modify, show]"));
        assert!(help.contains("relation"));
        assert!(help.contains("[class, object]"));
        assert!(help.contains("use next to fetch the next page"));
        assert!(help.contains("repl.enter_fetches_next_page"));
    }

    #[test]
    fn nested_scope_help_lists_generated_children() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog.render_scope_help(&["relation".to_string()]);

        assert!(help.contains("class"));
        assert!(help.contains("object"));
        assert!(help.contains("[create, delete, direct, graph, list, show]"));
        assert!(help.contains("[create, delete, direct, graph, list, show]"));
    }

    #[test]
    fn list_command_help_includes_where_guide() {
        let catalog = crate::commands::build_command_catalog();
        let help = catalog
            .render_command_help(&["object".to_string(), "list".to_string()])
            .expect("help should render");

        assert!(help.contains("Where:"));
        assert!(help.contains("Syntax: --where 'field operator value'"));
        assert!(help.contains("Repeat --where to combine filters with AND."));
        assert!(help.contains("json_data.<path>"));
        assert!(help.contains("data.<path>"));
        assert!(help.contains("json_data.contact equals Entry"));
        assert!(help.contains("Pagination:"));
        assert!(help.contains("Use --limit <n> to control page size."));
        assert!(help.contains("Use --cursor <token> to continue from a previous page."));
        assert!(help.contains("type next"));
        assert!(help.contains("repl.enter_fetches_next_page"));
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
}
