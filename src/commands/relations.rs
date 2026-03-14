use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{build_list_query, desired_format, lte_clause, render_list_page, CliCommand};
use crate::autocomplete::{
    classes, objects_from_class_a, objects_from_class_b, objects_from_root_class,
    relation_class_direct_sort, relation_class_direct_where, relation_class_graph_where,
    relation_class_list_sort, relation_class_list_where, relation_object_direct_sort,
    relation_object_direct_where, relation_object_graph_where, relation_object_sort,
    relation_object_where,
};
use crate::catalog::CommandCatalogBuilder;
use crate::domain::{ResolvedRelatedClassGraph, ResolvedRelatedObjectGraph};
use crate::errors::AppError;
use crate::formatting::{append_json, append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{AppServices, RelatedObjectOptions, RelationRoot, RelationTarget};
use crate::tokenizer::CommandTokenizer;

const DEFAULT_RELATED_OBJECT_MAX_DEPTH: i32 = 2;
const DEFAULT_RELATED_CLASS_MAX_DEPTH: i32 = 2;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["relation", "class"],
            catalog_command(
                "list",
                RelatedClassList::default(),
                CommandDocs {
                    about: Some("List classes related to one root class"),
                    long_about: Some(
                        "List classes related to a root class, with traversal filters like depth.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "class"],
            catalog_command(
                "show",
                ClassRelationShow::default(),
                CommandDocs {
                    about: Some("Show a class relation"),
                    long_about: Some(
                        "Show a direct class relation by id, or resolve it from an unordered class pair.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "class"],
            catalog_command(
                "create",
                ClassRelationCreate::default(),
                CommandDocs {
                    about: Some("Create a class relation"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "class"],
            catalog_command(
                "delete",
                ClassRelationDelete::default(),
                CommandDocs {
                    about: Some("Delete a class relation"),
                    long_about: Some(
                        "Delete a class relation by id, or resolve it from an unordered class pair.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "class"],
            catalog_command(
                "direct",
                RelatedClassRelationList::default(),
                CommandDocs {
                    about: Some("List direct relations touching one class"),
                    long_about: Some(
                        "List the direct class relations that touch a specific root class.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "class"],
            catalog_command(
                "graph",
                RelatedClassGraphCommand::default(),
                CommandDocs {
                    about: Some("Show the class neighborhood graph"),
                    long_about: Some(
                        "Fetch the connected-class neighborhood graph for a root class.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "list",
                RelatedObjectList::default(),
                CommandDocs {
                    about: Some("List objects related to one root object"),
                    long_about: Some(
                        "List objects related to a root object, with traversal filters like depth and ignore-class.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "show",
                ObjectRelationShowV2::default(),
                CommandDocs {
                    about: Some("Show an object relation"),
                    long_about: Some(
                        "Show an object relation by id, or resolve it from an unordered object pair.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "create",
                ObjectRelationCreateV2::default(),
                CommandDocs {
                    about: Some("Create an object relation"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "delete",
                ObjectRelationDeleteV2::default(),
                CommandDocs {
                    about: Some("Delete an object relation"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "direct",
                RelatedRelationList::default(),
                CommandDocs {
                    about: Some("List direct relations touching one object"),
                    long_about: Some(
                        "List the direct relations that touch a specific root object.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["relation", "object"],
            catalog_command(
                "graph",
                RelatedObjectGraphCommand::default(),
                CommandDocs {
                    about: Some("Show the object neighborhood graph"),
                    long_about: Some(
                        "Fetch the connected-object neighborhood graph for a root object.",
                    ),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedClassList {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include (defaults to 2)"
    )]
    pub max_depth: Option<i32>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_class_list_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "relation_class_list_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for RelatedClassList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            Some(lte_clause(
                "depth",
                query
                    .max_depth
                    .unwrap_or(DEFAULT_RELATED_CLASS_MAX_DEPTH)
                    .to_string(),
            )),
        )?;
        let classes = services
            .gateway()
            .list_related_classes(&query.root_class, &list_query)?;
        render_list_page(tokens, &classes)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassRelationShow {
    #[option(long = "id", help = "Class relation id")]
    pub id: Option<i32>,
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: Option<String>,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: Option<String>,
}

impl CliCommand for ClassRelationShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let relation = match query.id {
            Some(id) => services.gateway().get_class_relation_by_id(id)?,
            None => services.gateway().get_class_relation_by_pair(
                required_option(query.class_a, "class-a")?.as_str(),
                required_option(query.class_b, "class-b")?.as_str(),
            )?,
        };

        match desired_format(tokens) {
            OutputFormat::Json => relation.format_json_noreturn()?,
            OutputFormat::Text => relation.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassRelationCreate {
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: String,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: String,
}

impl CliCommand for ClassRelationCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let relation = services
            .gateway()
            .create_class_relation_v2(&query.class_a, &query.class_b)?;

        match desired_format(tokens) {
            OutputFormat::Json => relation.format_json_noreturn()?,
            OutputFormat::Text => relation.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassRelationDelete {
    #[option(long = "id", help = "Class relation id")]
    pub id: Option<i32>,
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: Option<String>,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: Option<String>,
}

impl CliCommand for ClassRelationDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let message = if let Some(id) = query.id {
            services.gateway().delete_class_relation_by_id(id)?;
            format!("Deleted class relation #{id}")
        } else {
            let class_a = required_option(query.class_a, "class-a")?;
            let class_b = required_option(query.class_b, "class-b")?;
            services
                .gateway()
                .delete_class_relation_by_pair(&class_a, &class_b)?;
            format!("Deleted class relation between '{class_a}' and '{class_b}'")
        };

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedClassRelationList {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_class_direct_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "relation_class_direct_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for RelatedClassRelationList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [],
        )?;
        let relations = services
            .gateway()
            .list_related_class_relations(&query.root_class, &list_query)?;
        render_list_page(tokens, &relations)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedClassGraphCommand {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include (defaults to 2)"
    )]
    pub max_depth: Option<i32>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_class_graph_where"
    )]
    pub where_clauses: Vec<String>,
}

impl CliCommand for RelatedClassGraphCommand {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let graph = services.gateway().related_class_graph(
            &query.root_class,
            &build_list_query(
                &query.where_clauses,
                &[],
                None,
                None,
                Some(lte_clause(
                    "depth",
                    query
                        .max_depth
                        .unwrap_or(DEFAULT_RELATED_CLASS_MAX_DEPTH)
                        .to_string(),
                )),
            )?
            .filters,
        )?;
        render_related_class_graph(tokens, &graph)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectRelationShowV2 {
    #[option(long = "id", help = "Object relation id")]
    pub id: Option<i32>,
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: Option<String>,
    #[option(
        long = "object-a",
        help = "First object endpoint",
        autocomplete = "objects_from_class_a"
    )]
    pub object_a: Option<String>,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: Option<String>,
    #[option(
        long = "object-b",
        help = "Second object endpoint",
        autocomplete = "objects_from_class_b"
    )]
    pub object_b: Option<String>,
}

impl CliCommand for ObjectRelationShowV2 {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let relation = match query.id {
            Some(id) => services.gateway().get_object_relation_by_id(id)?,
            None => services.gateway().get_object_relation_v2(
                &exact_object_target(query.class_a, query.object_a, query.class_b, query.object_b)?
                    .ok_or_else(|| {
                        AppError::MissingOptions(vec![
                            "class-a".to_string(),
                            "object-a".to_string(),
                            "class-b".to_string(),
                            "object-b".to_string(),
                        ])
                    })?,
            )?,
        };

        match desired_format(tokens) {
            OutputFormat::Json => relation.format_json_noreturn()?,
            OutputFormat::Text => relation.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectRelationCreateV2 {
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: String,
    #[option(
        long = "object-a",
        help = "First object endpoint",
        autocomplete = "objects_from_class_a"
    )]
    pub object_a: String,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: String,
    #[option(
        long = "object-b",
        help = "Second object endpoint",
        autocomplete = "objects_from_class_b"
    )]
    pub object_b: String,
}

impl CliCommand for ObjectRelationCreateV2 {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let relation = services
            .gateway()
            .create_object_relation_v2(&RelationTarget {
                class_a: query.class_a,
                class_b: query.class_b,
                object_a: Some(query.object_a),
                object_b: Some(query.object_b),
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => relation.format_json_noreturn()?,
            OutputFormat::Text => relation.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectRelationDeleteV2 {
    #[option(long = "id", help = "Object relation id")]
    pub id: Option<i32>,
    #[option(
        long = "class-a",
        help = "First class endpoint",
        autocomplete = "classes"
    )]
    pub class_a: Option<String>,
    #[option(
        long = "object-a",
        help = "First object endpoint",
        autocomplete = "objects_from_class_a"
    )]
    pub object_a: Option<String>,
    #[option(
        long = "class-b",
        help = "Second class endpoint",
        autocomplete = "classes"
    )]
    pub class_b: Option<String>,
    #[option(
        long = "object-b",
        help = "Second object endpoint",
        autocomplete = "objects_from_class_b"
    )]
    pub object_b: Option<String>,
}

impl CliCommand for ObjectRelationDeleteV2 {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let message = if let Some(id) = query.id {
            services.gateway().delete_object_relation_by_id(id)?;
            format!("Deleted object relation #{id}")
        } else {
            let target =
                exact_object_target(query.class_a, query.object_a, query.class_b, query.object_b)?
                    .ok_or_else(|| {
                        AppError::MissingOptions(vec![
                            "class-a".to_string(),
                            "object-a".to_string(),
                            "class-b".to_string(),
                            "object-b".to_string(),
                        ])
                    })?;
            services.gateway().delete_object_relation_v2(&target)?;
            format!(
                "Deleted object relation between '{}:{}' and '{}:{}'",
                target.class_a,
                target.object_a.clone().unwrap_or_default(),
                target.class_b,
                target.object_b.clone().unwrap_or_default()
            )
        };

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedRelationList {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "root-object",
        help = "Root object",
        autocomplete = "objects_from_root_class"
    )]
    pub root_object: String,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_object_direct_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "relation_object_direct_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for RelatedRelationList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            [],
        )?;
        let relations = services.gateway().list_related_object_relations(
            &RelationRoot {
                root_class: query.root_class,
                root_object: query.root_object,
            },
            &list_query,
        )?;
        render_list_page(tokens, &relations)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedObjectList {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "root-object",
        help = "Root object",
        autocomplete = "objects_from_root_class"
    )]
    pub root_object: String,
    #[option(
        long = "ignore-class",
        help = "Exclude returned objects in this class",
        autocomplete = "classes"
    )]
    pub ignore_class: Vec<String>,
    #[option(
        long = "include-self-class",
        help = "Include returned objects in the same class as the root object",
        flag = "true"
    )]
    pub include_self_class: Option<bool>,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include (defaults to 2)"
    )]
    pub max_depth: Option<i32>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_object_where"
    )]
    pub where_clauses: Vec<String>,
    #[option(
        long = "sort",
        help = "Sort clause: 'field asc|desc'",
        nargs = 2,
        autocomplete = "relation_object_sort"
    )]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for RelatedObjectList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            Some(lte_clause(
                "depth",
                query
                    .max_depth
                    .unwrap_or(DEFAULT_RELATED_OBJECT_MAX_DEPTH)
                    .to_string(),
            )),
        )?;
        let objects = services.gateway().list_related_objects(
            &RelationRoot {
                root_class: query.root_class,
                root_object: query.root_object,
            },
            &RelatedObjectOptions {
                ignore_classes: query.ignore_class,
                include_self_class: query.include_self_class.unwrap_or(false),
            },
            &list_query,
        )?;
        render_list_page(tokens, &objects)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RelatedObjectGraphCommand {
    #[option(long = "root-class", help = "Root class", autocomplete = "classes")]
    pub root_class: String,
    #[option(
        long = "root-object",
        help = "Root object",
        autocomplete = "objects_from_root_class"
    )]
    pub root_object: String,
    #[option(
        long = "max-depth",
        help = "Maximum traversal depth to include (defaults to 2)"
    )]
    pub max_depth: Option<i32>,
    #[option(
        long = "where",
        help = "Filter clause: 'field op value'",
        nargs = 3,
        autocomplete = "relation_object_graph_where"
    )]
    pub where_clauses: Vec<String>,
}

impl CliCommand for RelatedObjectGraphCommand {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let graph = services.gateway().related_object_graph(
            &RelationRoot {
                root_class: query.root_class,
                root_object: query.root_object,
            },
            &build_list_query(
                &query.where_clauses,
                &[],
                None,
                None,
                Some(lte_clause(
                    "depth",
                    query
                        .max_depth
                        .unwrap_or(DEFAULT_RELATED_OBJECT_MAX_DEPTH)
                        .to_string(),
                )),
            )?
            .filters,
        )?;
        render_related_object_graph(tokens, &graph)
    }
}

fn required_option(value: Option<String>, name: &str) -> Result<String, AppError> {
    value.ok_or_else(|| AppError::MissingOptions(vec![name.to_string()]))
}

fn exact_object_target(
    class_a: Option<String>,
    object_a: Option<String>,
    class_b: Option<String>,
    object_b: Option<String>,
) -> Result<Option<RelationTarget>, AppError> {
    match (class_a, object_a, class_b, object_b) {
        (None, None, None, None) => Ok(None),
        (Some(class_a), Some(object_a), Some(class_b), Some(object_b)) => {
            Ok(Some(RelationTarget {
                class_a,
                class_b,
                object_a: Some(object_a),
                object_b: Some(object_b),
            }))
        }
        _ => Err(AppError::MissingOptions(vec![
            "class-a".to_string(),
            "object-a".to_string(),
            "class-b".to_string(),
            "object-b".to_string(),
        ])),
    }
}

fn render_related_object_graph(
    tokens: &CommandTokenizer,
    graph: &ResolvedRelatedObjectGraph,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(graph)?,
        OutputFormat::Text => {
            append_line("Objects")?;
            graph.objects.format_noreturn()?;
            append_line("")?;
            append_line("Relations")?;
            graph.relations.format_noreturn()?;
        }
    }
    Ok(())
}

fn render_related_class_graph(
    tokens: &CommandTokenizer,
    graph: &ResolvedRelatedClassGraph,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => append_json(graph)?,
        OutputFormat::Text => {
            append_line("Classes")?;
            graph.classes.format_noreturn()?;
            append_line("")?;
            append_line("Relations")?;
            graph.relations.format_noreturn()?;
        }
    }
    Ok(())
}
