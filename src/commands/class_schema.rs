use std::str::FromStr;

use cli_command_derive::CommandArgs;
use hubuum_client::{
    ComplianceStatus, SchemaActivationPolicy, SchemaActivationRequest, SchemaObjectUrlTemplate,
    SchemaPageOptions, SchemaRepairReportRequest, SchemaRevision, SchemaStageRequest,
};
use serde_json::Value;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, render_format, required_option_or_pos, CliCommand};
use crate::autocomplete::{bool, classes};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::domain::SchemaOutput;
use crate::errors::{AppError, ReauthenticationRetry};
use crate::formatting::append_json;
use crate::models::OutputFormat;
use crate::output::{append_line, append_lines, has_pipeline, RenderFormat};
use crate::services::{AppServices, SchemaOperation};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.set_scope_help(
        &["class", "schema"],
        "Enable validation using the active schema (commands within this scope):\n\
         \x20 show Hosts\n\
         \x20 stage Hosts --validate true\n\
         \x20 impact Hosts --revision 2\n\
         \x20 work Hosts --task 123\n\
         \x20 activate Hosts --revision 2 --expected-active-revision 1 --impact-task 123\n\n\
         Use returned revision/task IDs. Poll work until complete and inspect readiness.\n\
         Staging and impact do not change the active policy. Use --validate false to disable\n\
         validation, or add --schema file://schema.json to stage a replacement schema.\n\
         Restore an earlier policy: stage Hosts --from-revision 4, then impact and activate\n\
         the new revision. Add --validate true|false to override the copied setting.",
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "show",
            SchemaShow::default(),
            CommandDocs {
                about: Some("Show active schema and compliance counts"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "revisions",
            SchemaRevisions::default(),
            CommandDocs {
                about: Some("List immutable schema revisions"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "objects",
            SchemaObjects::default(),
            CommandDocs {
                about: Some("List object compliance; follow next_after even on empty pages"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "stage",
            SchemaStage::default(),
            CommandDocs {
                about: Some("Stage a complete schema policy without activating it"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "revision",
            SchemaRevisionShow::default(),
            CommandDocs {
                about: Some("Show an immutable revision"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "abandon",
            SchemaAbandon::default(),
            CommandDocs {
                about: Some("Abandon a staged revision"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "impact",
            SchemaImpact::default(),
            CommandDocs {
                about: Some("Queue impact analysis for a staged revision"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "revalidate",
            SchemaRevalidate::default(),
            CommandDocs {
                about: Some("Queue revalidation of the active revision"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "activate",
            SchemaActivate::default(),
            CommandDocs {
                about: Some("Explicitly activate a staged revision"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "work",
            SchemaWork::default(),
            CommandDocs {
                about: Some("Show schema work progress and retained diagnostics"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "cancel",
            SchemaCancel::default(),
            CommandDocs {
                about: Some("Request cancellation of schema work"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "report",
            SchemaReport::default(),
            CommandDocs {
                about: Some("Fetch retained HTML repair report"),
                ..CommandDocs::default()
            },
        ),
    );
    builder.add_command(
        &["class", "schema"],
        catalog_command(
            "generate-report",
            SchemaGenerateReport::default(),
            CommandDocs {
                about: Some("Generate and retain an HTML repair report"),
                ..CommandDocs::default()
            },
        ),
    );
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaShow {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
}
impl CliCommand for SchemaShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Show;
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaRevisions {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "after", help = "Resume after this revision or object ID")]
    after: Option<i64>,
    #[option(long = "limit", help = "Page size (1-100; default 50)")]
    limit: Option<usize>,
}
impl CliCommand for SchemaRevisions {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Revisions(page_options(query.after, query.limit)?);
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaObjects {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "after", help = "Resume after this revision or object ID")]
    after: Option<i64>,
    #[option(long = "limit", help = "Page size (1-100; default 50)")]
    limit: Option<usize>,
    #[option(long = "status", help = "valid, invalid, pending, or not_required")]
    status: Option<String>,
}
impl CliCommand for SchemaObjects {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Objects(
            page_options(query.after, query.limit)?,
            parse_status(query.status.as_deref())?,
        );
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaStage {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(
        long = "schema",
        help = "Replacement JSON schema; omit to reuse the active schema, or use null to remove it",
        value_source = true
    )]
    schema: Option<Value>,
    #[option(
        long = "validate",
        help = "Whether the policy enforces validation",
        autocomplete = "bool"
    )]
    validate: Option<bool>,
    #[option(
        long = "from-revision",
        help = "Copy a previous schema policy into a new staged revision; optionally override --validate"
    )]
    from_revision: Option<i64>,
}
impl SchemaStage {
    fn operation(&self) -> Result<SchemaOperation, AppError> {
        if let Some(revision) = self.from_revision {
            if self.schema.is_some() {
                return Err(AppError::InvalidOption(
                    "Use either --from-revision or --schema, not both".into(),
                ));
            }
            return Ok(SchemaOperation::StageFromRevision {
                revision: SchemaRevision::new(revision)?,
                validate_schema: self.validate,
            });
        }
        let validate_schema = self.validate.ok_or_else(|| {
            AppError::MissingOptions(vec!["validate (or use --from-revision)".into()])
        })?;
        Ok(match &self.schema {
            Some(schema) => SchemaOperation::Stage(SchemaStageRequest {
                json_schema: (!schema.is_null()).then(|| schema.clone()),
                validate_schema,
            }),
            None => SchemaOperation::StageValidation { validate_schema },
        })
    }
}
impl CliCommand for SchemaStage {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let operation = query.operation()?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaRevisionShow {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "revision", help = "Immutable schema revision")]
    revision: i64,
}
impl CliCommand for SchemaRevisionShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Revision(SchemaRevision::new(query.revision)?);
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaAbandon {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "revision", help = "Immutable schema revision")]
    revision: i64,
}
impl CliCommand for SchemaAbandon {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Abandon(SchemaRevision::new(query.revision)?);
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaImpact {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "revision", help = "Immutable schema revision")]
    revision: i64,
}
impl CliCommand for SchemaImpact {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Impact(SchemaRevision::new(query.revision)?);
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaRevalidate {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "revision", help = "Immutable schema revision")]
    revision: i64,
}
impl CliCommand for SchemaRevalidate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Revalidate(SchemaRevision::new(query.revision)?);
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaActivate {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "revision", help = "Immutable schema revision")]
    revision: i64,
    #[option(
        long = "expected-active-revision",
        help = "Expected current active schema revision"
    )]
    expected_active_revision: i64,
    #[option(long = "impact-task", help = "Completed impact analysis task ID")]
    impact_task: Option<i32>,
    #[option(
        long = "allow-pending",
        help = "Allow pending or invalid existing objects (unscoped administrator only)",
        flag = true
    )]
    allow_pending: Option<bool>,
}
impl CliCommand for SchemaActivate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Activate(
            SchemaRevision::new(query.revision)?,
            SchemaActivationRequest {
                expected_active_revision: SchemaRevision::new(query.expected_active_revision)?,
                impact_task_id: query.impact_task.map(Into::into),
                policy: if query.allow_pending.unwrap_or(false) {
                    SchemaActivationPolicy::AllowPending
                } else {
                    SchemaActivationPolicy::RejectIncompatible
                },
            },
        );
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaWork {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "task", help = "Schema work task ID")]
    task: i32,
}
impl CliCommand for SchemaWork {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Work(query.task.into());
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaCancel {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "task", help = "Schema work task ID")]
    task: i32,
}
impl CliCommand for SchemaCancel {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let operation = SchemaOperation::Cancel(query.task.into());
        render_schema_output(
            tokens,
            &services.gateway().schema_operation(&class, operation)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaReport {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "task", help = "Schema work task ID")]
    task: i32,
}
impl CliCommand for SchemaReport {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let request = None;
        let html = services
            .gateway()
            .schema_report(&class, query.task.into(), request)?;
        match desired_format(tokens)? {
            OutputFormat::Json => append_json(&html)?,
            OutputFormat::Text => append_line(html)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
struct SchemaGenerateReport {
    #[option(
        short = "c",
        long = "class",
        help = "Class name",
        autocomplete = "classes"
    )]
    class: Option<String>,
    #[option(long = "task", help = "Schema work task ID")]
    task: i32,
    #[option(
        long = "object-url-template",
        help = "HTTP(S) object link containing {object_id}"
    )]
    object_url_template: String,
    #[option(long = "template-id", help = "Stored HTML layout template ID")]
    template_id: Option<i32>,
}
impl CliCommand for SchemaGenerateReport {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let class = required_option_or_pos(query.class, tokens, 0, "class")?;
        let request = Some(SchemaRepairReportRequest {
            object_url_template: SchemaObjectUrlTemplate::new(query.object_url_template)?,
            template_id: query.template_id.map(Into::into),
        });
        let html = services
            .gateway()
            .schema_report(&class, query.task.into(), request)?;
        match desired_format(tokens)? {
            OutputFormat::Json => append_json(&html)?,
            OutputFormat::Text => append_line(html)?,
        }
        Ok(())
    }
}
fn render_schema_output(tokens: &CommandTokenizer, result: &SchemaOutput) -> Result<(), AppError> {
    if has_pipeline()? || render_format(tokens)? != RenderFormat::Text {
        append_json(result)
    } else {
        append_lines(&result.summary_lines())
    }
}

fn page_options(after: Option<i64>, limit: Option<usize>) -> Result<SchemaPageOptions, AppError> {
    let mut page = SchemaPageOptions::default();
    if let Some(after) = after {
        page = page.after(after)?;
    }
    if let Some(limit) = limit {
        page = page.limit(limit)?;
    }
    Ok(page)
}
fn parse_status(value: Option<&str>) -> Result<Option<ComplianceStatus>, AppError> {
    value
        .map(|value| match ComplianceStatus::from_str(value) {
            Ok(
                status @ (ComplianceStatus::Valid
                | ComplianceStatus::Invalid
                | ComplianceStatus::Pending
                | ComplianceStatus::NotRequired),
            ) => Ok(status),
            _ => Err(AppError::InvalidOption(
                "Compliance status must be valid, invalid, pending, or not_required".into(),
            )),
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandArgs;

    #[test]
    #[serial_test::serial]
    fn text_summarizes_work_but_json_and_pipelines_keep_full_diagnostics() {
        use crate::commands::command_options;
        use crate::output::{reset_output, set_pipeline, set_render_format, take_output};
        use hubuum_client::SchemaWorkResponse;
        use hubuum_filter::Pipeline;
        use serde_json::{from_str, Value};

        let work: SchemaWorkResponse =
            from_str(include_str!("../../tests/fixtures/schema-work.json")).unwrap();
        let output = SchemaOutput::Work(Box::new(work));
        for (args, format, pipeline) in [
            ("work Hosts --task 123", RenderFormat::Text, false),
            (
                "work Hosts --task 123 --output json",
                RenderFormat::Json,
                false,
            ),
            ("work Hosts --task 123", RenderFormat::Json, true),
        ] {
            reset_output().unwrap();
            set_render_format(format).unwrap();
            if pipeline {
                set_pipeline(Pipeline::parse("P impact.findings").unwrap().into_stages()).unwrap();
            }
            let tokens =
                CommandTokenizer::new(args, "work", &command_options::<SchemaWork>()).unwrap();
            render_schema_output(&tokens, &output).unwrap();
            let rendered = take_output().unwrap().lines.join("\n");
            if format == RenderFormat::Text {
                assert!(
                    rendered
                        .lines()
                        .any(|line| line.starts_with("Readiness ")
                            && line.ends_with(": incompatible"))
                );
                assert!(!rendered.contains("\"snapshot\""));
            } else {
                let value: Value = from_str(&rendered).unwrap();
                assert!(value.to_string().contains("snapshot"));
                if !pipeline {
                    assert_eq!(value["impact"]["findings"][0]["object_id"], 11);
                }
            }
        }
    }

    #[test]
    fn schema_pages_reject_out_of_range_bounds_and_unknown_statuses() {
        assert!(page_options(Some(-1), None).is_err());
        assert!(page_options(None, Some(0)).is_err());
        assert!(page_options(None, Some(101)).is_err());
        assert!(page_options(Some(0), Some(100)).is_ok());
        assert!(parse_status(Some("unknown")).is_err());
        assert!(parse_status(Some("valid")).is_ok());
    }

    #[test]
    fn staging_requires_validation_but_distinguishes_reuse_from_removal() {
        for input in ["stage --class Hosts", "stage --class Hosts --schema null"] {
            let tokens = CommandTokenizer::new(input, "stage", &SchemaStage::options()).unwrap();
            assert!(
                SchemaStage::parse_tokens(&tokens)
                    .unwrap()
                    .operation()
                    .is_err(),
                "{input}"
            );
        }
        let tokens = CommandTokenizer::new(
            "stage --class Hosts --schema null --validate false",
            "stage",
            &SchemaStage::options(),
        )
        .unwrap();
        let policy = SchemaStage::parse_tokens(&tokens).unwrap();
        assert_eq!(policy.schema, Some(Value::Null));
        let tokens = CommandTokenizer::new(
            "stage --class Hosts --validate true",
            "stage",
            &SchemaStage::options(),
        )
        .unwrap();
        let reused = SchemaStage::parse_tokens(&tokens).unwrap();
        assert!(reused.schema.is_none());
        assert_eq!(reused.validate, Some(true));
        assert_eq!(policy.validate, Some(false));
    }

    #[test]
    fn copying_a_revision_preserves_validation_unless_explicitly_overridden() {
        let parse = |input| {
            let tokens = CommandTokenizer::new(input, "stage", &SchemaStage::options()).unwrap();
            SchemaStage::parse_tokens(&tokens).unwrap().operation()
        };
        assert!(matches!(
            parse("stage Hosts --from-revision 4").unwrap(),
            SchemaOperation::StageFromRevision {
                validate_schema: None,
                ..
            }
        ));
        assert!(matches!(
            parse("stage Hosts --from-revision 4 --validate false").unwrap(),
            SchemaOperation::StageFromRevision {
                validate_schema: Some(false),
                ..
            }
        ));
        assert!(parse("stage Hosts --from-revision 0").is_err());
        assert!(parse("stage Hosts --from-revision 4 --schema null").is_err());
    }

    #[test]
    fn activation_requires_the_expected_active_revision_and_defaults_to_strict() {
        let options = SchemaActivate::options();
        let tokens =
            CommandTokenizer::new("activate --class Hosts --revision 2", "activate", &options)
                .unwrap();
        assert!(SchemaActivate::parse_tokens(&tokens).is_err());
        let tokens = CommandTokenizer::new(
            "activate --class Hosts --revision 2 --expected-active-revision 1",
            "activate",
            &options,
        )
        .unwrap();
        let activation = SchemaActivate::parse_tokens(&tokens).unwrap();
        assert!(!activation.allow_pending.unwrap_or(false));
    }
}
