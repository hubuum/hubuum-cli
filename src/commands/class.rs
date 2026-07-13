use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};
use serde_json::{to_string_pretty, Value};

use super::builder::{catalog_command, CommandDocs};
use super::{
    build_list_query, contains_clause, desired_format, render_list_page, required_option_or_pos,
    CliCommand,
};
use crate::catalog::CommandCatalogBuilder;

use crate::autocomplete::{bool, class_sort, class_where, classes, collections};
use crate::config::get_config;
use crate::domain::ClassShowRecord;
use crate::errors::AppError;
use crate::formatting::{append_json_message, render_related_class_tree_with_key, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line};
use crate::services::{AppServices, ClassUpdateInput, CreateClassInput, RelationTraversalOptions};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["class"],
            catalog_command(
                "create",
                ClassNew::default(),
                CommandDocs {
                    about: Some("Create a new class"),
                    long_about: Some("Create a new class with the specified properties."),
                    examples: Some(
                        r#"-n MyClass -N collection_1 -d "My class description"
--name MyClass --collection collection_1 --description 'My class' --schema '{\"type\": \"object\"}'"#,
                    ),
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "list",
                ClassList::default(),
                CommandDocs {
                    about: Some("List classes"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "delete",
                ClassDelete::default(),
                CommandDocs {
                    about: Some("Delete a class"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "show",
                ClassInfo::default(),
                CommandDocs {
                    about: Some("Show class details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["class"],
            catalog_command(
                "modify",
                ClassModify::default(),
                CommandDocs {
                    about: Some("Modify a class"),
                    long_about: Some("Update an existing class by name."),
                    examples: Some(
                        r#"modify my-class --rename new-class
modify --name my-class --description "Updated description" --collection other-ns"#,
                    ),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassNew {
    #[option(short = "n", long = "name", help = "Name of the class")]
    pub name: String,
    #[option(
        short = "N",
        long = "collection",
        help = "Collection name",
        autocomplete = "collections"
    )]
    pub collection: String,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: String,
    #[option(
        short = "s",
        long = "schema",
        help = "JSON schema for the class",
        value_source = true
    )]
    pub json_schema: Option<Value>,
    #[option(
        short = "v",
        long = "validate",
        help = "Validate against schema, requires schema to be set",
        autocomplete = "bool"
    )]
    pub validate_schema: Option<bool>,
}

impl CliCommand for ClassNew {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;
        let result = services.gateway().create_class(CreateClassInput {
            name: new.name,
            collection: new.collection,
            description: new.description,
            json_schema: new.json_schema,
            validate_schema: new.validate_schema,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => result.format_json_noreturn()?,
            OutputFormat::Text => result.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassInfo {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
    #[option(
        long = "include-self-class",
        help = "Include returned relations in the same class as the root class",
        flag = "true"
    )]
    pub include_self_class: Option<bool>,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include in related class output"
    )]
    pub max_depth: Option<i32>,
}

impl CliCommand for ClassInfo {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;
        let config = get_config();
        let details = services.gateway().class_show_details(
            &name,
            &RelationTraversalOptions {
                include_self_class: query
                    .include_self_class
                    .unwrap_or(!config.relations.ignore_same_class),
                max_depth: query.max_depth.unwrap_or(config.relations.max_depth),
            },
        )?;

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(to_string_pretty(&details)?)?;
            }
            OutputFormat::Text => {
                render_class_show_text(&details)?;
            }
        }

        Ok(())
    }
}

fn render_class_show_text(details: &ClassShowRecord) -> Result<(), AppError> {
    details.class.format()?;
    let relation_padding = get_config().output.padding.saturating_sub(1);
    render_related_class_tree_with_key("Relations", &details.related_classes, relation_padding)?;
    append_key_value("Objects", details.objects.len(), 14)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassDelete {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
}

impl CliCommand for ClassDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        services.gateway().delete_class(&name)?;

        let message = format!("Class '{name}' deleted successfully");

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassModify {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
    #[option(short = "r", long = "rename", help = "Rename the class")]
    pub rename: Option<String>,
    #[option(
        short = "N",
        long = "collection",
        help = "Move the class to another collection",
        autocomplete = "collections"
    )]
    pub collection: Option<String>,
    #[option(
        short = "d",
        long = "description",
        help = "New description of the class"
    )]
    pub description: Option<String>,
    #[option(
        short = "s",
        long = "schema",
        help = "JSON schema for the class",
        value_source = true
    )]
    pub json_schema: Option<Value>,
    #[option(
        short = "v",
        long = "validate",
        help = "Set schema validation",
        autocomplete = "bool"
    )]
    pub validate_schema: Option<bool>,
}

impl CliCommand for ClassModify {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let name = required_option_or_pos(query.name, tokens, 0, "name")?;

        let updated = services.gateway().update_class(ClassUpdateInput {
            name,
            rename: query.rename,
            collection: query.collection,
            description: query.description,
            json_schema: query.json_schema,
            validate_schema: query.validate_schema,
        })?;

        match desired_format(tokens) {
            OutputFormat::Json => updated.format_json_noreturn()?,
            OutputFormat::Text => updated.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassList {
    #[option(
        short = "n",
        long = "name",
        help = "Name of the class",
        autocomplete = "classes"
    )]
    pub name: Option<String>,
    #[option(short = "d", long = "description", help = "Description of the class")]
    pub description: Option<String>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "class_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "class_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
}

impl CliCommand for ClassList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            query.include_total.unwrap_or(false),
            [
                query.name.map(|value| contains_clause("name", value)),
                query
                    .description
                    .map(|value| contains_clause("description", value)),
            ]
            .into_iter()
            .flatten(),
        )?;
        let classes = services.gateway().list_classes(&list_query)?;
        render_list_page(tokens, &classes)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json};
    use serial_test::serial;

    use super::render_class_show_text;
    use crate::domain::{ClassRecord, ClassShowRecord, RelatedClassTreeNode};
    use crate::output::{reset_output, take_output};

    #[test]
    #[serial]
    fn class_show_renders_relations_before_object_summary() {
        reset_output().expect("output should reset");
        let details = ClassShowRecord {
            class: ClassRecord(
                from_value(json!({
                    "id": 1,
                    "name": "Jacks",
                    "description": "",
                    "collection": {
                        "id": 1,
                        "name": "default",
                        "description": "",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z"
                    },
                    "json_schema": {},
                    "validate_schema": false,
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:00:00Z"
                }))
                .expect("class fixture should deserialize"),
            ),
            objects: vec![],
            related_classes: vec![RelatedClassTreeNode {
                id: 2,
                name: "Rooms".to_string(),
                collection: "default".to_string(),
                depth: 1,
                children: vec![],
            }],
        };

        render_class_show_text(&details).expect("class show text should render");

        let snapshot = take_output().expect("snapshot should exist");
        let relations_index = snapshot
            .lines
            .iter()
            .position(|line| line.starts_with("Relations"))
            .expect("relations line should exist");
        let objects_index = snapshot
            .lines
            .iter()
            .position(|line| line.starts_with("Objects"))
            .expect("object summary should exist");

        assert!(relations_index < objects_index);
        assert!(snapshot.lines.iter().any(|line| line.contains("Rooms")));
    }
}
