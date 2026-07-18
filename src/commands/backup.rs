use std::fs::{read_to_string, OpenOptions};
use std::io::Write;
use std::path::Path;

use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use serde_json::{json, to_value};

use super::admin::render_structured_value;
use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{run_task_backed, TaskSubmitOptions};
use super::{desired_format, option_or_pos, render_task_record, CliCommand};
use crate::autocomplete::{bool, file_paths};
use crate::catalog::CommandCatalogBuilder;
use crate::domain::{BackupArtifact, RestoreReceipt};
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line, set_semantic_output};
use crate::services::{AppServices, BackupInput, RunBackupInput};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["backup"],
            catalog_command(
                "create",
                BackupCreate::default(),
                CommandDocs {
                    about: Some("Create and securely save a full-system backup"),
                    long_about: Some(
                        "Submit an administrator-only backup, wait for completion, and save the versioned JSON document. Backup files can contain credentials and are created with owner-only permissions on Unix.",
                    ),
                    examples: Some("--file hubuum-backup.json\n--file hubuum-backup.json --include-history false"),
                },
            ),
        )
        .add_command(
            &["backup"],
            catalog_command(
                "submit",
                BackupSubmit::default(),
                CommandDocs {
                    about: Some("Submit a full-system backup task"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["backup"],
            catalog_command(
                "show",
                BackupShow::default(),
                CommandDocs {
                    about: Some("Show a backup task"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["backup"],
            catalog_command(
                "download",
                BackupDownload::default(),
                CommandDocs {
                    about: Some("Securely save a completed backup task's output"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["restore"],
            catalog_command(
                "stage",
                RestoreStage::default(),
                CommandDocs {
                    about: Some("Validate and stage a full-system restore"),
                    long_about: Some(
                        "Validate a backup document and save the one-time restore capability in an owner-only receipt file. Staging does not replace server data.",
                    ),
                    examples: Some("--file hubuum-backup.json --receipt restore-receipt.json"),
                },
            ),
        )
        .add_command(
            &["restore"],
            catalog_command(
                "status",
                RestoreStatus::default(),
                CommandDocs {
                    about: Some("Inspect a staged restore using its receipt"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["restore"],
            catalog_command(
                "confirm",
                RestoreConfirm::default(),
                CommandDocs {
                    about: Some("Confirm a destructive full-system restore"),
                    long_about: Some(
                        "Destructively replace all Hubuum data with the staged backup. Existing bearer tokens are invalidated. --yes is required.",
                    ),
                    examples: Some("--receipt restore-receipt.json --yes"),
                },
            ),
        );
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct BackupCreate {
    #[option(
        short = "f",
        long = "file",
        help = "Destination backup JSON file",
        autocomplete = "file_paths"
    )]
    file: String,
    #[option(
        long = "include-history",
        help = "Include history rows (default: true)",
        autocomplete = "bool"
    )]
    include_history: Option<bool>,
    #[option(long = "idempotency-key", help = "Optional idempotency key")]
    idempotency_key: Option<String>,
    #[option(long = "timeout", help = "Wait timeout in seconds (default: 300)")]
    timeout: Option<u64>,
    #[option(
        long = "poll-interval",
        help = "Task polling interval in seconds (default: 1)"
    )]
    poll_interval: Option<u64>,
    #[option(
        long = "force",
        help = "Replace an existing destination file",
        flag = true
    )]
    force: bool,
}

impl CliCommand for BackupCreate {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        ensure_output_available(&query.file, query.force)?;
        let backup = BackupInput::new(query.include_history.unwrap_or(true))
            .idempotency_key(query.idempotency_key);
        let artifact = services.gateway().run_backup(
            RunBackupInput::new(backup)
                .timeout_secs(query.timeout.or(Some(300)))
                .poll_interval_secs(query.poll_interval.or(Some(1))),
        )?;
        save_backup(&query.file, &artifact, query.force)?;
        render_backup_saved(tokens, &query.file, &artifact)
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct BackupSubmit {
    #[option(
        long = "include-history",
        help = "Include history rows (default: true)",
        autocomplete = "bool"
    )]
    include_history: Option<bool>,
    #[option(long = "idempotency-key", help = "Optional idempotency key")]
    idempotency_key: Option<String>,
}

impl CliCommand for BackupSubmit {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let task = services.gateway().submit_backup(
            BackupInput::new(query.include_history.unwrap_or(true))
                .idempotency_key(query.idempotency_key),
        )?;
        run_task_backed(
            services,
            tokens,
            format!("backup {}", task.0.id),
            TaskSubmitOptions::default(),
            task,
        )
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct BackupShow {
    #[option(long = "task", help = "Backup task ID")]
    task: Option<i32>,
}

impl CliCommand for BackupShow {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.task = option_or_pos(query.task, tokens, 0, "task")?;
        let task = services.gateway().backup_task(
            query
                .task
                .ok_or_else(|| AppError::MissingOptions(vec!["task".to_string()]))?,
        )?;
        render_task_record(tokens, &task)
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct BackupDownload {
    #[option(long = "task", help = "Completed backup task ID")]
    task: Option<i32>,
    #[option(
        short = "f",
        long = "file",
        help = "Destination backup JSON file",
        autocomplete = "file_paths"
    )]
    file: String,
    #[option(
        long = "force",
        help = "Replace an existing destination file",
        flag = true
    )]
    force: bool,
}

impl CliCommand for BackupDownload {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.task = option_or_pos(query.task, tokens, 0, "task")?;
        ensure_output_available(&query.file, query.force)?;
        let artifact = services.gateway().backup_output(
            query
                .task
                .ok_or_else(|| AppError::MissingOptions(vec!["task".to_string()]))?,
        )?;
        save_backup(&query.file, &artifact, query.force)?;
        render_backup_saved(tokens, &query.file, &artifact)
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct RestoreStage {
    #[option(
        short = "f",
        long = "file",
        help = "Backup JSON document",
        autocomplete = "file_paths"
    )]
    file: String,
    #[option(
        short = "r",
        long = "receipt",
        help = "Destination for the one-time restore receipt",
        autocomplete = "file_paths"
    )]
    receipt: String,
    #[option(long = "force", help = "Replace an existing receipt file", flag = true)]
    force: bool,
}

impl CliCommand for RestoreStage {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        ensure_output_available(&query.receipt, query.force)?;
        let backup_json = read_to_string(&query.file)?;
        let (record, receipt) = services.gateway().stage_restore(&backup_json)?;
        write_sensitive_file(&query.receipt, &receipt.json_pretty()?, query.force)?;
        let mut value = to_value(record)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("receipt_file".to_string(), json!(query.receipt));
        }
        render_structured_value(value, desired_format(tokens))
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct RestoreStatus {
    #[option(
        short = "r",
        long = "receipt",
        help = "Restore receipt file",
        autocomplete = "file_paths"
    )]
    receipt: String,
}

impl CliCommand for RestoreStatus {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        let receipt = load_receipt(&query.receipt)?;
        let status = services.gateway().restore_status(&receipt)?;
        render_structured_value(to_value(status)?, desired_format(tokens))
    }
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct RestoreConfirm {
    #[option(
        short = "r",
        long = "receipt",
        help = "Restore receipt file",
        autocomplete = "file_paths"
    )]
    receipt: String,
    #[option(
        long = "yes",
        help = "Confirm replacement of all Hubuum data",
        flag = true
    )]
    yes: bool,
}

impl CliCommand for RestoreConfirm {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let query = Self::parse_tokens(tokens)?;
        if !query.yes {
            return Err(AppError::InvalidOption(
                "Restore confirmation requires --yes because it replaces all Hubuum data"
                    .to_string(),
            ));
        }
        let receipt = load_receipt(&query.receipt)?;
        let status = services.gateway().confirm_restore(&receipt)?;
        render_structured_value(to_value(status)?, desired_format(tokens))
    }
}

fn save_backup(path: &str, artifact: &BackupArtifact, force: bool) -> Result<(), AppError> {
    write_sensitive_file(path, &artifact.json_pretty()?, force)
}

fn render_backup_saved(
    tokens: &CommandTokenizer,
    path: &str,
    artifact: &BackupArtifact,
) -> Result<(), AppError> {
    match desired_format(tokens) {
        OutputFormat::Json => set_semantic_output(OutputEnvelope::detail(
            json!({"file": path, "backup": artifact.summary()}),
            Vec::new(),
        ))?,
        OutputFormat::Text => {
            append_line(format!("Backup saved securely to {path}"))?;
            append_key_value("Version", artifact.summary().backup_version, 12)?;
            append_key_value("Source", &artifact.summary().source_version, 12)?;
            append_key_value("Created", &artifact.summary().created_at, 12)?;
            append_key_value("History", artifact.summary().includes_history, 12)?;
        }
    }
    Ok(())
}

fn load_receipt(path: &str) -> Result<RestoreReceipt, AppError> {
    RestoreReceipt::from_json(&read_to_string(path)?)
}

fn ensure_output_available(path: &str, force: bool) -> Result<(), AppError> {
    if Path::new(path).exists() && !force {
        return Err(AppError::InvalidOption(format!(
            "Destination '{path}' already exists; use --force to replace it"
        )));
    }
    Ok(())
}

fn write_sensitive_file(path: &str, contents: &str, force: bool) -> Result<(), AppError> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        options.mode(0o600);
        let mut file = options.open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }

    #[cfg(not(unix))]
    {
        let mut file = options.open(path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{read_to_string, write};

    use tempfile::tempdir;

    use super::{ensure_output_available, write_sensitive_file};

    #[test]
    fn sensitive_files_do_not_overwrite_without_force() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("backup.json");
        write(&path, "existing").expect("fixture");

        assert!(ensure_output_available(path.to_str().expect("path"), false).is_err());
        assert!(write_sensitive_file(path.to_str().expect("path"), "replacement", false).is_err());
        assert_eq!(read_to_string(path).expect("read"), "existing");
    }

    #[test]
    fn force_allows_replacing_sensitive_files() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("receipt.json");
        write(&path, "old").expect("fixture");

        write_sensitive_file(path.to_str().expect("path"), "new", true).expect("replace");
        assert_eq!(read_to_string(path).expect("read"), "new");
    }

    #[cfg(unix)]
    #[test]
    fn sensitive_files_are_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("backup.json");
        write_sensitive_file(path.to_str().expect("path"), "{}", false).expect("write");
        let mode = path.metadata().expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
