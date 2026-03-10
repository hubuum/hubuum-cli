use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, desired_format, equals_clause, render_list_page, CliCommand};
use crate::autocomplete::{
    classes, objects_from_class_from, objects_from_class_to, relation_sort, relation_where,
};
use crate::catalog::CommandCatalogBuilder;
use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::{OutputFormat, Relation};
use crate::output::append_line;
use crate::services::{AppServices, RelationTarget};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["relation"],
            catalog_command(
                "create",
                RelationNew::default(),
                CommandDocs {
                    about: Some("Create a relationship"),
                    long_about: Some("Create a new relationship between classes or objects."),
                    examples: Some(
                        r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#,
                    ),
                },
            ),
        )
        .add_command(
            &["relation"],
            catalog_command(
                "list",
                RelationList::default(),
                CommandDocs {
                    about: Some("List relationships"),
                    long_about: Some("List relationships between classes or objects."),
                    examples: Some(
                        r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#,
                    ),
                },
            ),
        )
        .add_command(
            &["relation"],
            catalog_command(
                "delete",
                RelationDelete::default(),
                CommandDocs {
                    about: Some("Delete a relationship"),
                    long_about: Some("Delete a new relationship between classes or objects."),
                    examples: Some(
                        r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#,
                    ),
                },
            ),
        )
        .add_command(
            &["relation"],
            catalog_command(
                "show",
                RelationInfo::default(),
                CommandDocs {
                    about: Some("Information about a relationships"),
                    long_about: Some(
                        "Show information about relationships between classes or objects.",
                    ),
                    examples: Some(
                        r#"--class_from FromClass --class_to ToClass
    --class_from FromClass --class_to ToClass --object_from FromObject --object_to ToObject
    "#,
                    ),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelationNew {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelationDelete {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelationList {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: Option<String>,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: Option<String>,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "relation_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelationInfo {
    #[option(
        short = "f",
        long = "class_from",
        help = "Name of the class the relationship starts from",
        autocomplete = "classes"
    )]
    pub class_from: String,
    #[option(
        short = "t",
        long = "class_to",
        help = "Name of the class the relationship goes to",
        autocomplete = "classes"
    )]
    pub class_to: String,
    #[option(
        short = "F",
        long = "object_from",
        help = "Name of the object the relationship starts from",
        autocomplete = "objects_from_class_from"
    )]
    pub object_from: Option<String>,
    #[option(
        short = "T",
        long = "object_to",
        help = "Name of the object the relationship goes to",
        autocomplete = "objects_from_class_to"
    )]
    pub object_to: Option<String>,
}

impl CliCommand for RelationNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let rel: Relation = if new.object_from.is_none() && new.object_to.is_none() {
            services
                .gateway()
                .create_class_relation(&new.class_from, &new.class_to)?
                .into()
        } else {
            services
                .gateway()
                .create_object_relation(&RelationTarget {
                    class_from: new.class_from.clone(),
                    class_to: new.class_to.clone(),
                    object_from: new.object_from.clone(),
                    object_to: new.object_to.clone(),
                })?
                .into()
        };

        match desired_format(tokens) {
            OutputFormat::Json => rel.format_json_noreturn()?,
            OutputFormat::Text => rel.format_noreturn()?,
        };

        Ok(())
    }
}

impl CliCommand for RelationDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;

        if new.object_from.is_none() && new.object_to.is_none() {
            services
                .gateway()
                .delete_class_relation(&new.class_from, &new.class_to)?;
        } else {
            services.gateway().delete_object_relation(&RelationTarget {
                class_from: new.class_from.clone(),
                class_to: new.class_to.clone(),
                object_from: new.object_from.clone(),
                object_to: new.object_to.clone(),
            })?;
        }

        let message = match (new.object_from.as_ref(), new.object_to.as_ref()) {
            (None, None) => format!(
                "Deleted class relation from '{}' to '{}'",
                new.class_from, new.class_to
            ),
            (Some(from), Some(to)) => format!(
                "Deleted object relation from '{}' to '{}' in classes '{}' and '{}'",
                from, to, new.class_from, new.class_to
            ),
            (None, Some(_)) => {
                return Err(AppError::MissingOptions(vec!["object_from".to_string()]))
            }
            (Some(_), None) => return Err(AppError::MissingOptions(vec!["object_to".to_string()])),
        };

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        };

        Ok(())
    }
}

impl CliCommand for RelationList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [
                query
                    .class_from
                    .map(|value| equals_clause("class_from", value)),
                query.class_to.map(|value| equals_clause("class_to", value)),
                query
                    .object_from
                    .map(|value| equals_clause("object_from", value)),
                query
                    .object_to
                    .map(|value| equals_clause("object_to", value)),
            ]
            .into_iter()
            .flatten(),
        )?;

        let has_object_filters = list_query
            .filters
            .iter()
            .any(|clause| matches!(clause.field.as_str(), "object_from" | "object_to"));
        let has_both_class_filters = list_query
            .filters
            .iter()
            .any(|clause| clause.field == "class_from")
            && list_query
                .filters
                .iter()
                .any(|clause| clause.field == "class_to");
        let wants_object_relations = has_object_filters || has_both_class_filters;

        if wants_object_relations {
            let relations = services.gateway().list_object_relations(&list_query)?;
            render_list_page(tokens, &relations)
        } else {
            let relations = services.gateway().list_class_relations(&list_query)?;
            render_list_page(tokens, &relations)
        }
    }
}

impl CliCommand for RelationInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        if new.object_from.is_none() && new.object_to.is_none() {
            let rel = services
                .gateway()
                .get_class_relation(&new.class_from, &new.class_to)?;

            match desired_format(tokens) {
                OutputFormat::Json => rel.format_json_noreturn()?,
                OutputFormat::Text => rel.format_noreturn()?,
            }
        } else {
            let object_relation = services.gateway().get_object_relation(&RelationTarget {
                class_from: new.class_from.clone(),
                class_to: new.class_to.clone(),
                object_from: new.object_from.clone(),
                object_to: new.object_to.clone(),
            })?;

            match desired_format(tokens) {
                OutputFormat::Json => object_relation.format_json_noreturn()?,
                OutputFormat::Text => object_relation.format_noreturn()?,
            }
        }
        Ok(())
    }
}
