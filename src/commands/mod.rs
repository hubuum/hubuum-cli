use log::trace;
use serde::Serialize;
use serde_json::to_string_pretty;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::Display;
use std::str::FromStr;

use hubuum_client::FilterOperator;

mod audit;
mod builder;
mod class;
mod collection;
pub(crate) mod config;
mod event_delivery;
mod event_sink;
mod event_subscription;
mod export;
mod group;
mod help;
mod history;
mod imports;
mod jobs;
mod me;
mod object;
mod relations;
mod remote_target;
mod search;
mod service_account;
mod task;
mod task_submit;
pub(crate) mod theme;
mod user;

pub use builder::build_command_catalog;

use crate::autocomplete::output_formats;
use crate::domain::{JsonRecord, TaskRecord};
use crate::output::RenderFormat;
use crate::services::CompletionContext;
use crate::suggestions::did_you_mean_message;
use crate::{errors::AppError, services::AppServices, tokenizer::CommandTokenizer};
use crate::{
    formatting::{OutputFormatter, TableRenderable},
    list_query::{
        filter_clause, list_query_from_raw, render_paged_result, FilterClause, ListQuery,
        PagedResult,
    },
    models::OutputFormat,
    output::append_line,
};

pub type AutoCompleter = fn(&CompletionContext, &str, &[String]) -> Vec<String>;

#[allow(dead_code)]
#[derive(Debug)]
pub struct CliOption {
    pub name: String,
    pub short: Option<String>,
    pub long: Option<String>,
    pub flag: bool,
    pub greedy: bool,
    pub nargs: Option<usize>,
    pub repeatable: bool,
    pub value_source: bool,
    pub help: String,
    pub field_type: TypeId,
    pub field_type_help: String,
    pub required: bool,
    pub autocomplete: Option<AutoCompleter>,
}

impl CliOption {
    pub fn short_without_dash(&self) -> Option<String> {
        self.short.as_ref().map(|s| s[1..].to_string())
    }

    pub fn long_without_dashes(&self) -> Option<String> {
        self.long.as_ref().map(|l| l[2..].to_string())
    }
}

pub trait CommandArgs: Sized + Default + Send + Sync + 'static {
    fn options() -> Vec<CliOption>;

    fn parse_tokens(tokens: &CommandTokenizer) -> Result<Self, AppError>;
}

pub trait CliCommand: CommandArgs + Send + Sync {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError>;
}

pub fn standard_options() -> Vec<CliOption> {
    vec![
        CliOption {
            name: "help".to_string(),
            short: Some("-h".to_string()),
            long: Some("--help".to_string()),
            flag: true,
            greedy: false,
            nargs: None,
            repeatable: false,
            value_source: false,
            help: "Prints help information".to_string(),
            field_type: TypeId::of::<bool>(),
            field_type_help: "bool".to_string(),
            required: false,
            autocomplete: None,
        },
        CliOption {
            name: "json".to_string(),
            short: Some("-j".to_string()),
            long: Some("--json".to_string()),
            flag: true,
            greedy: false,
            nargs: None,
            repeatable: false,
            value_source: false,
            help: "Output as JSON".to_string(),
            field_type: TypeId::of::<bool>(),
            field_type_help: "bool".to_string(),
            required: false,
            autocomplete: None,
        },
        CliOption {
            name: "output".to_string(),
            short: Some("-o".to_string()),
            long: Some("--output".to_string()),
            flag: false,
            greedy: false,
            nargs: None,
            repeatable: false,
            value_source: false,
            help: "Output format: text, json, jsonl, csv, or tsv".to_string(),
            field_type: TypeId::of::<String>(),
            field_type_help: "string".to_string(),
            required: false,
            autocomplete: Some(output_formats),
        },
    ]
}

pub fn command_options<C: CommandArgs>() -> Vec<CliOption> {
    let mut options = C::options();
    options.extend(standard_options());
    options
}

pub fn validate_command_args<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    validate_unknown_options::<C>(tokens)?;
    validate_not_both_short_and_long_set::<C>(tokens)?;
    validate_missing_options::<C>(tokens)?;
    validate_flag_options::<C>(tokens)?;
    validate_output_options(tokens)?;
    Ok(())
}

pub fn validate_unknown_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut known_options = HashSet::new();
    let mut known_display_options = Vec::new();
    for opt in command_options::<C>() {
        if let Some(short) = opt.short_without_dash() {
            known_display_options.push(format!("-{short}"));
            known_options.insert(short);
        }
        if let Some(long) = opt.long_without_dashes() {
            known_display_options.push(format!("--{long}"));
            known_options.insert(long);
        }
    }

    let mut unknown_options: Vec<String> = tokens
        .get_options()
        .keys()
        .filter(|key| !known_options.contains(*key))
        .map(|key| unknown_option_message(key, &known_display_options))
        .collect();

    if unknown_options.is_empty() {
        return Ok(());
    }

    unknown_options.sort();
    Err(AppError::InvalidOption(unknown_options.join(", ")))
}

fn unknown_option_message(key: &str, known_display_options: &[String]) -> String {
    let displayed = if key.len() == 1 {
        format!("-{key}")
    } else {
        format!("--{key}")
    };
    match did_you_mean_message(&displayed, known_display_options.to_vec()) {
        Some(hint) => format!("{displayed}. {hint}"),
        None => displayed,
    }
}

pub fn validate_missing_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut missing_options = Vec::new();

    for opt in command_options::<C>() {
        if !opt.required {
            continue;
        }

        if let Some(short) = &opt.short_without_dash() {
            if tokenpairs.contains_key(short) {
                continue;
            }
            trace!("Short not found: {short}");
        }
        if let Some(long) = &opt.long_without_dashes() {
            if tokenpairs.contains_key(long) {
                continue;
            }
            trace!("Long not found: {long}");
        }

        missing_options.push(opt.name.clone());
    }

    if !missing_options.is_empty() {
        return Err(AppError::MissingOptions(missing_options));
    }

    Ok(())
}

pub fn validate_not_both_short_and_long_set<C: CommandArgs>(
    tokens: &CommandTokenizer,
) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut duplicate_options = Vec::new();

    for opt in command_options::<C>() {
        if let Some(short) = opt.short_without_dash() {
            if let Some(long) = opt.long_without_dashes() {
                if tokenpairs.contains_key(&short) && tokenpairs.contains_key(&long) {
                    duplicate_options.push(opt.name.clone());
                }
            }
        }
    }

    if !duplicate_options.is_empty() {
        return Err(AppError::DuplicateOptions(duplicate_options));
    }

    Ok(())
}

/// Flag options are not allowed to have values, but are boolean flags. In the tokenizer
/// they are represented as a key with an empty ("") value. We alert if we find any flag
/// options with a value.
pub fn validate_flag_options<C: CommandArgs>(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let tokenpairs = tokens.get_options();
    let mut populated_flag_options = Vec::new();

    for opt in command_options::<C>() {
        if opt.flag {
            if let Some(short) = &opt.short_without_dash() {
                if let Some(value) = tokenpairs.get(short) {
                    if !value.is_empty() {
                        populated_flag_options.push(short.clone());
                    }
                }
            }
            if let Some(long) = &opt.long_without_dashes() {
                if let Some(value) = tokenpairs.get(long) {
                    if !value.is_empty() {
                        populated_flag_options.push(long.clone());
                    }
                }
            }
        }
    }

    if !populated_flag_options.is_empty() {
        return Err(AppError::PopulatedFlagOptions(populated_flag_options));
    }

    Ok(())
}

pub fn desired_format(tokens: &CommandTokenizer) -> OutputFormat {
    if want_json(tokens) || output_format_name(tokens).as_deref() == Some("json") {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    }
}

pub fn build_list_query(
    where_clauses: &[String],
    sort_clauses: &[String],
    limit: Option<usize>,
    cursor: Option<String>,
    compatibility_filters: impl IntoIterator<Item = FilterClause>,
) -> Result<ListQuery, AppError> {
    let mut query = list_query_from_raw(where_clauses, sort_clauses, limit, cursor)?;
    query.filters.extend(compatibility_filters);
    Ok(query)
}

pub fn render_list_page<T>(
    tokens: &CommandTokenizer,
    paged: &PagedResult<T>,
) -> Result<(), AppError>
where
    T: Serialize + Clone + TableRenderable,
{
    render_paged_result(tokens, paged, desired_format(tokens))
}

pub fn render_task_record(tokens: &CommandTokenizer, task: &TaskRecord) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_line(to_string_pretty(task)?)?,
        OutputFormat::Text => task.format_noreturn()?,
    }

    Ok(())
}

pub fn contains_clause(field: impl Into<String>, value: impl Into<String>) -> FilterClause {
    filter_clause(
        field,
        FilterOperator::IContains { is_negated: false },
        value,
    )
}

pub fn equals_clause(field: impl Into<String>, value: impl Into<String>) -> FilterClause {
    filter_clause(field, FilterOperator::Equals { is_negated: false }, value)
}

pub fn option_or_pos<T>(
    value: Option<T>,
    tokens: &CommandTokenizer,
    pos: usize,
    name: &str,
) -> Result<Option<T>, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    if value.is_some() {
        return Ok(value);
    }

    tokens
        .get_positionals()
        .get(pos)
        .map(|value| parse_positional(value, name))
        .transpose()
}

pub fn required_option_or_pos<T>(
    value: Option<T>,
    tokens: &CommandTokenizer,
    pos: usize,
    name: &str,
) -> Result<T, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    required_option(option_or_pos(value, tokens, pos, name)?, name)
}

pub fn first_positional_or<T>(
    value: Option<T>,
    tokens: &CommandTokenizer,
    name: &str,
) -> Result<Option<T>, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    option_or_pos(value, tokens, 0, name)
}

pub fn name_or_first_pos(name: Option<String>, tokens: &CommandTokenizer) -> Option<String> {
    name.or_else(|| tokens.get_positionals().first().cloned())
}

pub fn required_option<T>(value: Option<T>, name: &str) -> Result<T, AppError> {
    value.ok_or_else(|| AppError::MissingOptions(vec![name.to_string()]))
}

pub fn required_str<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, AppError> {
    value.ok_or_else(|| AppError::MissingOptions(vec![name.to_string()]))
}

pub fn required_i64(value: Option<i64>, name: &str) -> Result<i64, AppError> {
    required_option(value, name)
}

pub fn render_json_record(tokens: &CommandTokenizer, record: &JsonRecord) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => record.format_json_noreturn(),
        OutputFormat::Text => record.format_noreturn(),
    }
}

fn parse_positional<T>(value: &str, name: &str) -> Result<T, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    value
        .parse::<T>()
        .map_err(|err| AppError::ParseError(format!("{name} has invalid value '{value}': {err}")))
}

pub fn lte_clause(field: impl Into<String>, value: impl Into<String>) -> FilterClause {
    filter_clause(field, FilterOperator::Lte { is_negated: false }, value)
}

pub fn want_json(tokens: &CommandTokenizer) -> bool {
    let opts = tokens.get_options();
    opts.contains_key("j") || opts.contains_key("json")
}

pub fn output_format_name(tokens: &CommandTokenizer) -> Option<String> {
    let opts = tokens.get_options();
    opts.get("o")
        .or_else(|| opts.get("output"))
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

pub fn render_format(tokens: &CommandTokenizer) -> Result<RenderFormat, AppError> {
    if want_json(tokens) {
        return Ok(RenderFormat::Json);
    }

    match output_format_name(tokens).as_deref() {
        Some("text") | None => Ok(RenderFormat::Text),
        Some("json") => Ok(RenderFormat::Json),
        Some("jsonl") => Ok(RenderFormat::Jsonl),
        Some("csv") => Ok(RenderFormat::Csv),
        Some("tsv") => Ok(RenderFormat::Tsv),
        Some(other) => Err(AppError::ParseError(format!(
            "Unknown output format: {other}. Use text, json, jsonl, csv, or tsv."
        ))),
    }
}

fn validate_output_options(tokens: &CommandTokenizer) -> Result<(), AppError> {
    if want_json(tokens) {
        if let Some(format) = output_format_name(tokens) {
            if format != "json" {
                return Err(AppError::ParseError(
                    "--json conflicts with --output values other than json".to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub fn want_help(tokens: &CommandTokenizer) -> bool {
    let opts = tokens.get_options();
    opts.contains_key("h") || opts.contains_key("help")
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::{
        option_or_pos, required_option_or_pos, validate_unknown_options, CliOption, CommandArgs,
    };
    use crate::errors::AppError;
    use crate::tokenizer::CommandTokenizer;

    #[derive(Default)]
    struct DummyArgs;

    impl CommandArgs for DummyArgs {
        fn options() -> Vec<CliOption> {
            vec![CliOption {
                name: "limit".to_string(),
                short: None,
                long: Some("--limit".to_string()),
                flag: false,
                greedy: false,
                nargs: None,
                repeatable: false,
                value_source: false,
                help: "Limit".to_string(),
                field_type: TypeId::of::<usize>(),
                field_type_help: "usize".to_string(),
                required: false,
                autocomplete: None,
            }]
        }

        fn parse_tokens(_tokens: &CommandTokenizer) -> Result<Self, AppError> {
            Ok(Self)
        }
    }

    #[test]
    fn unknown_options_suggest_nearby_known_options() {
        let tokens = CommandTokenizer::new("dummy list --limt 10", "list", &[])
            .expect("unknown option tokenization should succeed");
        let err = validate_unknown_options::<DummyArgs>(&tokens)
            .expect_err("unknown option should fail validation");

        assert!(err.to_string().contains("Did you mean '--limit'?"));
    }

    #[test]
    fn option_or_pos_prefers_explicit_option() {
        let tokens =
            CommandTokenizer::new("dummy show positional", "show", &[]).expect("tokenization");

        let value = option_or_pos(Some("explicit".to_string()), &tokens, 0, "name")
            .expect("option should parse");

        assert_eq!(value.as_deref(), Some("explicit"));
    }

    #[test]
    fn required_option_or_pos_uses_positional_value() {
        let tokens =
            CommandTokenizer::new("dummy show positional", "show", &[]).expect("tokenization");

        let value: String =
            required_option_or_pos(None, &tokens, 0, "name").expect("positional should parse");

        assert_eq!(value, "positional");
    }

    #[test]
    fn required_option_or_pos_exports_missing_value() {
        let tokens = CommandTokenizer::new("dummy show", "show", &[]).expect("tokenization");

        let err = required_option_or_pos::<String>(None, &tokens, 0, "name")
            .expect_err("missing required value should fail");

        assert!(matches!(err, AppError::MissingOptions(options) if options == vec!["name"]));
    }

    #[test]
    fn option_or_pos_exports_invalid_positional_value() {
        let tokens = CommandTokenizer::new("dummy show nope", "show", &[]).expect("tokenization");

        let err = option_or_pos::<i64>(None, &tokens, 0, "id").expect_err("invalid id should fail");

        assert!(err.to_string().contains("id has invalid value 'nope'"));
    }
}
