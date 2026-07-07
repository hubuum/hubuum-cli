use cli_command_derive::CommandArgs;
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{parse_task_submit_options, run_task_backed};
use super::{build_list_query, desired_format, render_list_page, CliCommand};
use crate::autocomplete::{
    classes, namespaces, objects_from_class, objects_from_class_a, objects_from_class_b,
    remote_auth_types, remote_http_methods, remote_subject_kinds, remote_subject_types,
};
use crate::catalog::CommandCatalogBuilder;

use crate::errors::AppError;
use crate::formatting::{append_json_message, OutputFormatter};
use crate::models::OutputFormat;
use crate::output::append_line;
use crate::services::{
    AppServices, CreateRemoteTargetInput, InvokeRemoteTargetInput, RemoteAuthConfigInput,
    UpdateRemoteTargetInput,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["remote-target"],
            catalog_command(
                "create",
                RemoteTargetCreate::default(),
                CommandDocs {
                    about: Some("Create a remote target"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["remote-target"],
            catalog_command(
                "list",
                RemoteTargetList::default(),
                CommandDocs {
                    about: Some("List remote targets"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["remote-target"],
            catalog_command(
                "show",
                RemoteTargetShow::default(),
                CommandDocs {
                    about: Some("Show remote target details"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["remote-target"],
            catalog_command(
                "update",
                RemoteTargetUpdate::default(),
                CommandDocs {
                    about: Some("Update a remote target"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["remote-target"],
            catalog_command(
                "delete",
                RemoteTargetDelete::default(),
                CommandDocs {
                    about: Some("Delete a remote target"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["remote-target"],
            catalog_command(
                "invoke",
                RemoteTargetInvoke::default(),
                CommandDocs {
                    about: Some("Invoke a remote target"),
                    long_about: Some("Submit a remote target invocation as a background task. Use --wait to block until completion."),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetCreate {
    #[option(short = "n", long = "name", help = "Name of the remote target")]
    pub name: String,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: String,
    #[option(short = "d", long = "description", help = "Description")]
    pub description: String,
    #[option(
        short = "m",
        long = "method",
        help = "HTTP method (get, post, patch, delete)",
        autocomplete = "remote_http_methods"
    )]
    pub method: String,
    #[option(short = "u", long = "url", help = "URL template")]
    pub url_template: String,
    #[option(
        long = "subject-types",
        help = "Allowed subject types (comma-separated: namespace,class,object,class_relation,object_relation)",
        autocomplete = "remote_subject_types"
    )]
    pub allowed_subject_types: String,
    #[option(
        long = "auth-type",
        help = "Auth type: none, bearer, basic, apikey",
        autocomplete = "remote_auth_types"
    )]
    pub auth_type: Option<String>,
    #[option(long = "auth-secret", help = "Secret for bearer/basic/apikey auth")]
    pub auth_secret: Option<String>,
    #[option(long = "auth-username", help = "Username for basic auth")]
    pub auth_username: Option<String>,
    #[option(long = "auth-header", help = "Header name for apikey auth")]
    pub auth_header: Option<String>,
    #[option(
        long = "body-template",
        help = "Body template (JSON string)",
        value_source = true
    )]
    pub body_template: Option<String>,
    #[option(long = "class", help = "Class filter", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(long = "enabled", help = "Enabled flag", flag = true)]
    pub enabled: Option<bool>,
    #[option(
        long = "headers",
        help = "Headers template (JSON string)",
        value_source = true
    )]
    pub headers_template: Option<String>,
    #[option(long = "timeout-ms", help = "Timeout in milliseconds")]
    pub timeout_ms: Option<i32>,
}

impl CliCommand for RemoteTargetCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let new = Self::parse_tokens(tokens)?;

        let auth_config = parse_auth_config(
            new.auth_type,
            new.auth_secret,
            new.auth_username,
            new.auth_header,
        )?;

        let headers = new
            .headers_template
            .map(|h| serde_json::from_str(&h))
            .transpose()
            .map_err(|e| AppError::ParseError(format!("Invalid headers JSON: {}", e)))?;

        let subject_types = new
            .allowed_subject_types
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let target = services
            .gateway()
            .create_remote_target(CreateRemoteTargetInput {
                namespace: new.namespace,
                name: new.name,
                description: new.description,
                method: new.method,
                url_template: new.url_template,
                allowed_subject_types: subject_types,
                auth_config,
                body_template: new.body_template,
                class: new.class,
                enabled: new.enabled,
                headers_template: headers,
                timeout_ms: new.timeout_ms,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => target.format_json_noreturn()?,
            OutputFormat::Text => target.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetList {
    #[option(long = "where", help = "Filter clause: 'field op value'", nargs = 3)]
    pub where_clauses: Vec<String>,
    #[option(long = "sort", help = "Sort clause: 'field asc|desc'", nargs = 2)]
    pub sort_clauses: Vec<String>,
    #[option(long = "limit", help = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[option(long = "cursor", help = "Cursor for the next result page")]
    pub cursor: Option<String>,
}

impl CliCommand for RemoteTargetList {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let list_query = build_list_query(
            &query.where_clauses,
            &query.sort_clauses,
            query.limit,
            query.cursor,
            std::iter::empty(),
        )?;
        let targets = services.gateway().list_remote_targets(&list_query)?;
        render_list_page(tokens, &targets)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetShow {
    #[option(short = "n", long = "name", help = "Name of the remote target")]
    pub name: Option<String>,
}

impl CliCommand for RemoteTargetShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        if new.name.is_none() {
            if let Some(pos0) = tokens.get_positionals().first() {
                new.name = Some(pos0.clone());
            } else {
                return Err(AppError::MissingOptions(vec!["name".to_string()]));
            }
        }

        let target = services
            .gateway()
            .remote_target(new.name.as_ref().unwrap())?;

        match desired_format(tokens) {
            OutputFormat::Json => target.format_json_noreturn()?,
            OutputFormat::Text => target.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetUpdate {
    #[option(short = "n", long = "name", help = "Name of the remote target")]
    pub name: Option<String>,
    #[option(long = "rename", help = "New name")]
    pub rename: Option<String>,
    #[option(short = "d", long = "description", help = "Description")]
    pub description: Option<String>,
    #[option(
        long = "namespace",
        help = "Namespace name",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(
        short = "m",
        long = "method",
        help = "HTTP method (get, post, patch, delete)",
        autocomplete = "remote_http_methods"
    )]
    pub method: Option<String>,
    #[option(short = "u", long = "url", help = "URL template")]
    pub url_template: Option<String>,
    #[option(
        long = "subject-types",
        help = "Allowed subject types (comma-separated)",
        autocomplete = "remote_subject_types"
    )]
    pub allowed_subject_types: Option<String>,
    #[option(
        long = "auth-type",
        help = "Auth type: none, bearer, basic, apikey",
        autocomplete = "remote_auth_types"
    )]
    pub auth_type: Option<String>,
    #[option(long = "auth-secret", help = "Secret for bearer/basic/apikey auth")]
    pub auth_secret: Option<String>,
    #[option(long = "auth-username", help = "Username for basic auth")]
    pub auth_username: Option<String>,
    #[option(long = "auth-header", help = "Header name for apikey auth")]
    pub auth_header: Option<String>,
    #[option(
        long = "body-template",
        help = "Body template (JSON string)",
        value_source = true
    )]
    pub body_template: Option<String>,
    #[option(long = "class", help = "Class filter", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(long = "enabled", help = "Enabled flag", flag = true)]
    pub enabled: Option<bool>,
    #[option(
        long = "headers",
        help = "Headers template (JSON string)",
        value_source = true
    )]
    pub headers_template: Option<String>,
    #[option(long = "timeout-ms", help = "Timeout in milliseconds")]
    pub timeout_ms: Option<i32>,
}

impl CliCommand for RemoteTargetUpdate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;

        if query.name.is_none() {
            if let Some(pos0) = tokens.get_positionals().first() {
                query.name = Some(pos0.clone());
            } else {
                return Err(AppError::MissingOptions(vec!["name".to_string()]));
            }
        }

        let name = query
            .name
            .ok_or_else(|| AppError::MissingOptions(vec!["name".to_string()]))?;

        let auth_config = if query.auth_type.is_some()
            || query.auth_secret.is_some()
            || query.auth_username.is_some()
            || query.auth_header.is_some()
        {
            parse_auth_config(
                query.auth_type,
                query.auth_secret,
                query.auth_username,
                query.auth_header,
            )?
        } else {
            None
        };

        let headers = query
            .headers_template
            .map(|h| serde_json::from_str(&h))
            .transpose()
            .map_err(|e| AppError::ParseError(format!("Invalid headers JSON: {}", e)))?;

        let subject_types = query
            .allowed_subject_types
            .map(|types| types.split(',').map(|s| s.trim().to_string()).collect());

        let target = services
            .gateway()
            .update_remote_target(UpdateRemoteTargetInput {
                name,
                rename: query.rename,
                description: query.description,
                namespace: query.namespace,
                method: query.method,
                url_template: query.url_template,
                allowed_subject_types: subject_types,
                auth_config,
                body_template: query.body_template,
                class: query.class,
                enabled: query.enabled,
                headers_template: headers,
                timeout_ms: query.timeout_ms,
            })?;

        match desired_format(tokens) {
            OutputFormat::Json => target.format_json_noreturn()?,
            OutputFormat::Text => target.format_noreturn()?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetDelete {
    #[option(short = "n", long = "name", help = "Name of the remote target")]
    pub name: Option<String>,
}

impl CliCommand for RemoteTargetDelete {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        if new.name.is_none() {
            if let Some(pos0) = tokens.get_positionals().first() {
                new.name = Some(pos0.clone());
            } else {
                return Err(AppError::MissingOptions(vec!["name".to_string()]));
            }
        }

        let target_name = new.name.as_ref().unwrap().clone();
        services.gateway().delete_remote_target(&target_name)?;

        let message = format!("Remote target '{}' deleted", target_name);

        match desired_format(tokens) {
            OutputFormat::Json => append_json_message(&message)?,
            OutputFormat::Text => append_line(message)?,
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct RemoteTargetInvoke {
    #[option(short = "n", long = "name", help = "Name of the remote target")]
    pub name: Option<String>,
    #[option(
        long = "subject",
        help = "Subject kind: namespace, class, object, class_relation, object_relation",
        autocomplete = "remote_subject_kinds"
    )]
    pub subject_kind: String,
    #[option(
        long = "namespace",
        help = "Namespace subject",
        autocomplete = "namespaces"
    )]
    pub namespace: Option<String>,
    #[option(
        long = "class",
        help = "Class subject or object class",
        autocomplete = "classes"
    )]
    pub class: Option<String>,
    #[option(
        long = "object",
        help = "Object subject",
        autocomplete = "objects_from_class"
    )]
    pub object: Option<String>,
    #[option(
        long = "class-a",
        help = "First relation class",
        autocomplete = "classes"
    )]
    pub class_a: Option<String>,
    #[option(
        long = "class-b",
        help = "Second relation class",
        autocomplete = "classes"
    )]
    pub class_b: Option<String>,
    #[option(
        long = "object-a",
        help = "First relation object",
        autocomplete = "objects_from_class_a"
    )]
    pub object_a: Option<String>,
    #[option(
        long = "object-b",
        help = "Second relation object",
        autocomplete = "objects_from_class_b"
    )]
    pub object_b: Option<String>,
    #[option(long = "parameters", help = "Parameters JSON", value_source = true)]
    pub parameters: Option<String>,
    #[option(long = "body", help = "Body override JSON", value_source = true)]
    pub body_override: Option<String>,
    #[option(long = "wait", help = "Wait for task completion", flag = true)]
    pub wait: Option<bool>,
    #[option(long = "timeout", help = "Timeout in seconds for --wait")]
    pub timeout: Option<u64>,
    #[option(long = "poll-interval", help = "Poll interval in seconds for --wait")]
    pub poll_interval: Option<u64>,
}

impl CliCommand for RemoteTargetInvoke {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut new = Self::parse_tokens(tokens)?;

        if new.name.is_none() {
            if let Some(pos0) = tokens.get_positionals().first() {
                new.name = Some(pos0.clone());
            } else {
                return Err(AppError::MissingOptions(vec!["name".to_string()]));
            }
        }

        let parameters = new
            .parameters
            .map(|p| serde_json::from_str(&p))
            .transpose()
            .map_err(|e| AppError::ParseError(format!("Invalid parameters JSON: {}", e)))?;

        let body_override = new
            .body_override
            .map(|b| serde_json::from_str(&b))
            .transpose()
            .map_err(|e| AppError::ParseError(format!("Invalid body JSON: {}", e)))?;

        let task = services.gateway().invoke_remote_target(
            new.name.as_ref().unwrap(),
            InvokeRemoteTargetInput {
                subject_kind: new.subject_kind,
                namespace: new.namespace,
                class: new.class,
                object: new.object,
                class_a: new.class_a,
                class_b: new.class_b,
                object_a: new.object_a,
                object_b: new.object_b,
                parameters,
                body_override,
            },
        )?;

        let opts = parse_task_submit_options(tokens)?;
        run_task_backed(
            services,
            tokens,
            format!("remote-call {}", task.0.id),
            opts,
            task,
        )
    }
}

fn parse_auth_config(
    auth_type: Option<String>,
    auth_secret: Option<String>,
    auth_username: Option<String>,
    auth_header: Option<String>,
) -> Result<Option<RemoteAuthConfigInput>, AppError> {
    match auth_type.as_deref() {
        None => Ok(None),
        Some("none") => Ok(Some(RemoteAuthConfigInput::None)),
        Some("bearer") => {
            let secret = auth_secret
                .ok_or_else(|| AppError::MissingOptions(vec!["auth-secret".to_string()]))?;
            Ok(Some(RemoteAuthConfigInput::BearerSecret { secret }))
        }
        Some("basic") => {
            let username = auth_username
                .ok_or_else(|| AppError::MissingOptions(vec!["auth-username".to_string()]))?;
            let secret = auth_secret
                .ok_or_else(|| AppError::MissingOptions(vec!["auth-secret".to_string()]))?;
            Ok(Some(RemoteAuthConfigInput::BasicSecret {
                username,
                secret,
            }))
        }
        Some("apikey") => {
            let header = auth_header
                .ok_or_else(|| AppError::MissingOptions(vec!["auth-header".to_string()]))?;
            let secret = auth_secret
                .ok_or_else(|| AppError::MissingOptions(vec!["auth-secret".to_string()]))?;
            Ok(Some(RemoteAuthConfigInput::ApiKeySecret { header, secret }))
        }
        Some(other) => Err(AppError::ParseError(format!(
            "Invalid auth type '{}'. Valid values: none, bearer, basic, apikey",
            other
        ))),
    }
}
