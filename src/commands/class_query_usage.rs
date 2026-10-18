use cli_command_derive::CommandArgs;
use hubuum_client::{
    JsonPath, QueryUsageAnalysisRequest, QueryUsageOperation, QueryUsagePattern,
    QueryUsageReplaceRequest, QueryUsageValueType,
};
use serde_json::from_str;

use super::builder::{catalog_command, CommandDocs};
use super::{render_format, required_option, required_option_or_pos, CliCommand};
use crate::autocomplete::{classes, query_usage_operations, query_usage_types};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::domain::{ExpectedQueryUsageRevision, QueryUsageId, QueryUsageOutput};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::append_json;
use crate::output::{append_lines, has_pipeline, RenderFormat};
use crate::services::{AppServices, QueryUsageCommand};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.set_scope_help(&["class", "query-usage"], "Declare expected JSON query patterns independently of schema validation. Declarations do not guarantee indexes or faster queries. List requires ReadClass; mutations require UpdateClass. Analysis requires an unscoped administrator with class read access. Replacement and deletion require the revision you reviewed; conflicts are not retried. Native preparation runs separately on the server with hubuum-admin --reconcile-query-usage.");
    builder.add_command(&["class", "query-usage"], catalog_command("list", QueryUsageList::default(), docs("List declarations and current schema assessments", "Hosts")))
        .add_command(&["class", "query-usage"], catalog_command("create", QueryUsageCreate::default(), docs("Declare a JSON query pattern", "Hosts --path hardware,serial_number --type string --operations equals")))
        .add_command(&["class", "query-usage"], catalog_command("replace", QueryUsageReplace::default(), docs("Replace a declaration at its reviewed revision", "Hosts --id 1 --expected-revision 2 --path cpu,cores --type numeric --operations equals,gte")))
        .add_command(&["class", "query-usage"], catalog_command("delete", QueryUsageDelete::default(), docs("Delete a declaration at its reviewed revision", "Hosts --id 1 --expected-revision 2")))
        .add_command(&["class", "query-usage"], catalog_command("analyze", QueryUsageAnalyze::default(), docs("Review workload evidence, suggestions and native resources (administrator)", "Hosts\nHosts --proposed file://patterns.json")));
}

fn docs(about: &'static str, examples: &'static str) -> CommandDocs {
    CommandDocs {
        about: Some(about),
        examples: Some(examples),
        ..Default::default()
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct QueryUsageList {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
}

impl CliCommand for QueryUsageList {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        execute(services, tokens, query.class, QueryUsageCommand::List)
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct QueryUsageCreate {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(
        long = "path",
        help = "Comma-separated JSON path, e.g. hardware,serial_number"
    )]
    path: String,
    #[option(
        long = "type",
        help = "Scalar type: string, numeric, boolean",
        autocomplete = "query_usage_types"
    )]
    value_type: String,
    #[option(
        long = "operations",
        help = "Comma-separated operations: equals,gt,gte,lt,lte,between",
        autocomplete = "query_usage_operations"
    )]
    operations: String,
}

impl CliCommand for QueryUsageCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let pattern = pattern(&query.path, &query.value_type, &query.operations)?;
        execute(
            services,
            tokens,
            query.class,
            QueryUsageCommand::Create(pattern),
        )
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct QueryUsageReplace {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(long = "id", help = "Positive declaration ID")]
    id: Option<QueryUsageId>,
    #[option(
        long = "expected-revision",
        help = "Positive declaration revision you reviewed"
    )]
    expected_revision: Option<ExpectedQueryUsageRevision>,
    #[option(
        long = "path",
        help = "Comma-separated JSON path, e.g. hardware,serial_number"
    )]
    path: String,
    #[option(
        long = "type",
        help = "Scalar type: string, numeric, boolean",
        autocomplete = "query_usage_types"
    )]
    value_type: String,
    #[option(
        long = "operations",
        help = "Comma-separated operations: equals,gt,gte,lt,lte,between",
        autocomplete = "query_usage_operations"
    )]
    operations: String,
}

impl CliCommand for QueryUsageReplace {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let id = required_option(query.id, "id")?.get();
        let expected_revision =
            required_option(query.expected_revision, "expected-revision")?.get();
        let pattern = pattern(&query.path, &query.value_type, &query.operations)?;
        execute(
            services,
            tokens,
            query.class,
            QueryUsageCommand::Replace {
                id,
                request: QueryUsageReplaceRequest {
                    pattern,
                    expected_revision,
                },
            },
        )
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct QueryUsageDelete {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(long = "id", help = "Positive declaration ID")]
    id: Option<QueryUsageId>,
    #[option(
        long = "expected-revision",
        help = "Positive declaration revision you reviewed"
    )]
    expected_revision: Option<ExpectedQueryUsageRevision>,
}

impl CliCommand for QueryUsageDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let id = required_option(query.id, "id")?.get();
        let expected_revision =
            required_option(query.expected_revision, "expected-revision")?.get();
        execute(
            services,
            tokens,
            query.class,
            QueryUsageCommand::Delete {
                id,
                expected_revision,
            },
        )
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct QueryUsageAnalyze {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    class: Option<String>,
    #[option(
        long = "proposed",
        help = "JSON array of up to 32 hypothetical patterns; analysis does not adopt them",
        value_source = true
    )]
    proposed: Option<String>,
}

impl CliCommand for QueryUsageAnalyze {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let request = proposed_patterns(query.proposed.as_deref())?;
        execute(
            services,
            tokens,
            query.class,
            QueryUsageCommand::Analyze(request),
        )
    }
}

fn pattern(path: &str, value_type: &str, operations: &str) -> Result<QueryUsagePattern, AppError> {
    let value_type = match value_type {
        "string" => QueryUsageValueType::String,
        "numeric" => QueryUsageValueType::Numeric,
        "boolean" => QueryUsageValueType::Boolean,
        _ => {
            return Err(AppError::InvalidOption(
                "--type must be string, numeric, or boolean".into(),
            ))
        }
    };
    let operations = operations
        .split(',')
        .map(|operation| match operation.trim() {
            "equals" => Ok(QueryUsageOperation::Equals),
            "gt" => Ok(QueryUsageOperation::Gt),
            "gte" => Ok(QueryUsageOperation::Gte),
            "lt" => Ok(QueryUsageOperation::Lt),
            "lte" => Ok(QueryUsageOperation::Lte),
            "between" => Ok(QueryUsageOperation::Between),
            _ => Err(AppError::InvalidOption(
                "--operations must contain one to six of equals,gt,gte,lt,lte,between".into(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(QueryUsagePattern::new(
        JsonPath::new(path.split(','))?,
        value_type,
        operations,
    )?)
}

fn proposed_patterns(value: Option<&str>) -> Result<QueryUsageAnalysisRequest, AppError> {
    let patterns: Vec<QueryUsagePattern> = value.map(from_str).transpose()?.unwrap_or_default();
    Ok(QueryUsageAnalysisRequest::new(patterns)?)
}

fn execute(
    services: &AppServices,
    tokens: &CommandTokenizer,
    class: Option<String>,
    command: QueryUsageCommand,
) -> Result<(), AppError> {
    let class = required_option_or_pos(class, tokens, 0, "class")?;
    let gateway = services.gateway();
    render(tokens, &gateway.query_usage(&class, command)?)
}

fn render(tokens: &CommandTokenizer, result: &QueryUsageOutput) -> Result<(), AppError> {
    if has_pipeline()? || render_format(tokens)? != RenderFormat::Text {
        append_json(result)
    } else {
        append_lines(&result.summary_lines())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_validation_matches_the_server_vocabulary_and_bounds() {
        assert!(pattern("cpu,cores", "numeric", "equals,gte,between").is_ok());
        assert_eq!(
            pattern("serial", "string", "equals,equals")
                .unwrap()
                .operations()
                .len(),
            1
        );
        for (path, kind, operations) in [
            ("cpu.cores", "numeric", "gt"),
            ("a,,b", "string", "equals"),
            ("serial", "string", "gte"),
            ("flag", "boolean", "between"),
            ("serial", "string", ""),
            ("serial", "number", "equals"),
        ] {
            assert!(pattern(path, kind, operations).is_err());
        }
        assert!(pattern(&"a".repeat(513), "string", "equals").is_err());
        assert!(pattern(&vec!["a"; 33].join(","), "string", "equals").is_err());
    }

    #[test]
    fn proposals_are_bounded_and_do_not_accept_unvalidated_patterns() {
        assert!(proposed_patterns(None).unwrap().proposed().is_empty());
        assert!(proposed_patterns(Some(
            r#"[{"path":"x","value_type":"string","operations":["gt"]}]"#
        ))
        .is_err());
        let pattern = serde_json::json!({"path":"x","value_type":"string","operations":["equals"]});
        assert!(
            proposed_patterns(Some(&serde_json::to_string(&vec![pattern; 33]).unwrap())).is_err()
        );
    }

    #[test]
    fn declaration_ids_and_revisions_are_validated_before_requests() {
        for invalid in ["0", "-1", "abc", "999999999999999999999"] {
            assert!(invalid.parse::<QueryUsageId>().is_err());
            assert!(invalid.parse::<ExpectedQueryUsageRevision>().is_err());
        }
        assert_eq!("7".parse::<QueryUsageId>().unwrap().get().get(), 7);
        assert_eq!(
            "2".parse::<ExpectedQueryUsageRevision>()
                .unwrap()
                .get()
                .get(),
            2
        );
    }
}
