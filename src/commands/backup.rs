use std::fs::{read_to_string, symlink_metadata};
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};

use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use serde_json::{json, to_value, Value};
use tempfile::NamedTempFile;

use super::admin::render_structured_value;
use super::builder::{catalog_command, CommandDocs};
use super::task_submit::{run_task_backed, TaskSubmitOptions};
use super::{desired_format, option_or_pos, render_task_record, CliCommand};
use crate::app::{build_client, request_with_server_port_fallback};
use crate::autocomplete::{bool, file_paths};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::config::get_config;
use crate::domain::{BackupArtifact, RestoreReceipt, RestoreRecord, RestoreWaitOptions};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::models::OutputFormat;
use crate::output::{append_key_value, append_line, has_pipeline, set_semantic_output};
use crate::services::{AppServices, BackupInput, RestoreMonitor, RunBackupInput};
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
                    about: Some("Inspect a restore using its receipt without logging in"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["restore"],
            catalog_command(
                "wait",
                RestoreWait::default(),
                CommandDocs {
                    about: Some("Wait for restore completion without logging in"),
                    long_about: Some("Poll using the receipt capability until the restore succeeds, fails, or expires. A timeout leaves the restore running; reuse the receipt to resume monitoring. The matching hubuum-admin --restore-executor must be running on the server."),
                    examples: Some("--receipt restore-receipt.json --timeout 600"),
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
                        "Queue destructive replacement of all Hubuum data with the staged backup. --yes is required. Use --wait to wait for completion; a confirmed response only means the restore was queued. Existing bearer tokens are invalidated on completion. Reset a local administrator password with hubuum-admin --reset-password and issue fresh tokens after restore.",
                    ),
                    examples: Some("--receipt restore-receipt.json --yes --wait"),
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
        let destination = SensitiveOutput::new(&query.file, query.force)?;
        let backup = BackupInput::new(query.include_history.unwrap_or(true))
            .idempotency_key(query.idempotency_key);
        let artifact = services.gateway().run_backup(
            RunBackupInput::new(backup)
                .timeout_secs(query.timeout.or(Some(300)))
                .poll_interval_secs(query.poll_interval.or(Some(1))),
        )?;
        destination.write(&artifact.json_pretty()?)?;
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
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

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
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::Mutating;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.task = option_or_pos(query.task, tokens, 0, "task")?;
        let destination = SensitiveOutput::new(&query.file, query.force)?;
        let artifact = services.gateway().backup_output(
            query
                .task
                .ok_or_else(|| AppError::MissingOptions(vec!["task".to_string()]))?,
        )?;
        destination.write(&artifact.json_pretty()?)?;
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
        let destination = SensitiveOutput::new(&query.receipt, query.force)?;
        let backup_json = read_to_string(&query.file)?;
        let (record, receipt) = services.gateway().stage_restore(&backup_json)?;
        destination.write(&receipt.json_pretty()?)?;
        let mut value = to_value(record)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("receipt_file".to_string(), json!(query.receipt));
        }
        render_restore_value(value, tokens)
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
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_restore_status(tokens)
    }
}

pub(crate) fn render_restore_status(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = RestoreStatus::parse_tokens(tokens)?;
    let receipt = load_receipt(&query.receipt)?;
    let (_, status) = restore_monitor(&receipt)?;
    render_restore_value(to_value(status)?, tokens)
}

#[derive(Debug, Clone, CommandArgs, Default)]
pub struct RestoreWait {
    #[option(
        short = "r",
        long = "receipt",
        help = "Restore receipt file",
        autocomplete = "file_paths"
    )]
    receipt: String,
    #[option(long = "timeout", help = "Wait timeout in seconds (default: 300)")]
    timeout: Option<u64>,
    #[option(
        long = "poll-interval",
        help = "Polling interval in seconds (default: 1; minimum: 1)"
    )]
    poll_interval: Option<u64>,
}

impl CliCommand for RestoreWait {
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_restore_wait(tokens)
    }
}

pub(crate) fn render_restore_wait(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = RestoreWait::parse_tokens(tokens)?;
    let options = RestoreWaitOptions::new(query.timeout, query.poll_interval)?;
    let receipt = load_receipt(&query.receipt)?;
    let (monitor, status) = restore_monitor(&receipt)?;
    let mut first = Some(status);
    let status = options.wait(|| {
        first
            .take()
            .map(Ok)
            .unwrap_or_else(|| monitor.status(&receipt))
    })?;
    render_restore_value(to_value(status)?, tokens)
}

fn restore_monitor(receipt: &RestoreReceipt) -> Result<(RestoreMonitor, RestoreRecord), AppError> {
    request_with_server_port_fallback(get_config(), |config| {
        let monitor = RestoreMonitor::new(build_client(config)?);
        let status = monitor.status(receipt)?;
        Ok((monitor, status))
    })
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
    #[option(
        long = "wait",
        help = "Wait until the queued restore completes",
        flag = true
    )]
    wait: bool,
    #[option(
        long = "timeout",
        help = "Wait timeout in seconds with --wait (default: 300)"
    )]
    timeout: Option<u64>,
    #[option(
        long = "poll-interval",
        help = "Polling interval in seconds with --wait (default: 1; minimum: 1)"
    )]
    poll_interval: Option<u64>,
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
        if !query.wait && (query.timeout.is_some() || query.poll_interval.is_some()) {
            return Err(AppError::InvalidOption(
                "--timeout and --poll-interval require --wait".to_string(),
            ));
        }
        let options = RestoreWaitOptions::new(query.timeout, query.poll_interval)?;
        let receipt = load_receipt(&query.receipt)?;
        let mut status = services.gateway().confirm_restore(&receipt)?;
        if query.wait {
            let mut first = Some(status);
            status = options.wait(|| {
                first
                    .take()
                    .map(Ok)
                    .unwrap_or_else(|| services.gateway().restore_status(&receipt))
            })?;
        }
        render_restore_value(to_value(status)?, tokens)
    }
}

fn render_restore_value(value: Value, tokens: &CommandTokenizer) -> Result<(), AppError> {
    if has_pipeline()? {
        set_semantic_output(OutputEnvelope::detail(value, Vec::new()))
    } else {
        render_structured_value(value, desired_format(tokens)?)
    }
}

fn render_backup_saved(
    tokens: &CommandTokenizer,
    path: &str,
    artifact: &BackupArtifact,
) -> Result<(), AppError> {
    let format = if has_pipeline()? {
        OutputFormat::Json
    } else {
        desired_format(tokens)?
    };
    match format {
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
    if Path::new(path).file_name().is_none() {
        return Err(AppError::InvalidOption(
            "Destination must include a file name".to_string(),
        ));
    }
    match symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(AppError::InvalidOption(format!(
            "Destination '{path}' must be a regular file, not a directory, symbolic link, or device"
        )))
        }
        Ok(_) if !force => {
            return Err(AppError::InvalidOption(format!(
                "Destination '{path}' already exists; use --force to replace it"
            )))
        }
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    Ok(())
}

struct SensitiveOutput {
    path: PathBuf,
    temporary: NamedTempFile,
    force: bool,
}

impl SensitiveOutput {
    fn new(path: &str, force: bool) -> Result<Self, AppError> {
        ensure_output_available(path, force)?;
        let path = PathBuf::from(path);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Creating the owner-only temporary file before submitting remote work
        // catches inaccessible destinations before the one-time capability exists.
        let temporary = NamedTempFile::new_in(parent)?;
        Ok(Self {
            path,
            temporary,
            force,
        })
    }

    fn write(mut self, contents: &str) -> Result<(), AppError> {
        self.temporary.write_all(contents.as_bytes())?;
        self.temporary.as_file().sync_all()?;
        let result = if self.force {
            self.temporary.persist(&self.path)
        } else {
            self.temporary.persist_noclobber(&self.path)
        };
        if let Err(error) = result {
            let cause = error.error;
            let (_, recovery_path) = error.file.keep().map_err(|error| error.error)?;
            return Err(AppError::CommandExecutionError(format!(
                "Could not save '{}': {cause}. The complete sensitive file is preserved at '{}' for recovery.",
                self.path.display(), recovery_path.display()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{read_to_string, write};

    use tempfile::tempdir;

    use super::{ensure_output_available, SensitiveOutput};

    fn write_sensitive_file(
        path: &str,
        contents: &str,
        force: bool,
    ) -> Result<(), crate::errors::AppError> {
        SensitiveOutput::new(path, force)?.write(contents)
    }

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

    #[test]
    fn abandoning_a_prepared_write_preserves_the_old_backup() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("backup.json");
        write(&path, "last good backup").unwrap();
        let destination = SensitiveOutput::new(path.to_str().unwrap(), true).unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "last good backup");
        drop(destination);
        assert_eq!(read_to_string(&path).unwrap(), "last good backup");
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }

    #[test]
    fn a_racing_destination_is_not_clobbered_and_the_receipt_is_recoverable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        let destination = SensitiveOutput::new(path.to_str().unwrap(), false).unwrap();
        write(&path, "racing writer").unwrap();
        let error = destination
            .write("one-time receipt")
            .unwrap_err()
            .to_string();
        assert!(error.contains("preserved at"));
        assert!(!error.contains("one-time receipt"));
        assert_eq!(read_to_string(&path).unwrap(), "racing writer");
        let recovery = dir
            .path()
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|entry| entry != &path)
            .unwrap();
        assert_eq!(read_to_string(recovery).unwrap(), "one-time receipt");
    }

    #[test]
    fn inaccessible_parent_and_directory_destinations_fail_during_preparation() {
        let dir = tempdir().unwrap();
        assert!(SensitiveOutput::new("", true).is_err());
        assert!(SensitiveOutput::new(dir.path().to_str().unwrap(), true).is_err());
        assert!(SensitiveOutput::new(
            dir.path().join("missing/receipt.json").to_str().unwrap(),
            false
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn force_does_not_follow_symlinks_or_modify_hard_link_targets() {
        use std::fs::hard_link;
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        write(&target, "untouched").unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(SensitiveOutput::new(link.to_str().unwrap(), true).is_err());
        let hard = dir.path().join("hard");
        hard_link(&target, &hard).unwrap();
        write_sensitive_file(hard.to_str().unwrap(), "new", true).unwrap();
        assert_eq!(read_to_string(&target).unwrap(), "untouched");
        assert_eq!(read_to_string(hard).unwrap(), "new");
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
