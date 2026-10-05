use cli_command_derive::CommandArgs;
use hubuum_update::{UpdateMode, UpdateStatus, Updater};
use serde::Serialize;
use serde_json::json;

use super::builder::{catalog_command, CommandDocs};
use super::{render_json_record, CliCommand};
use crate::build_info;
use crate::catalog::CommandCatalogBuilder;
use crate::domain::JsonRecord;
use crate::errors::AppError;
use crate::services::AppServices;
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder.add_command(
        &[],
        catalog_command(
            "self-update",
            SelfUpdate::default(),
            CommandDocs {
                about: Some("Update this executable to the latest stable CLI release"),
                long_about: Some("Download the official release for this platform, verify its SHA-256 checksum, and replace the running executable on disk. No Hubuum login is required. Use --check to inspect availability without downloading or installing. The installation directory must be writable. Restart an active REPL after updating; the current process keeps running the old version. Package-managed installations should use their package manager. Rolling main builds and prereleases are not update destinations."),
                examples: Some("self-update --check\nself-update\nself-update --check --output json"),
            },
        ),
    );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct SelfUpdate {
    #[option(
        long = "check",
        help = "Check for a newer stable release without changing the executable",
        flag = true
    )]
    pub check: Option<bool>,
}

impl CliCommand for SelfUpdate {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_self_update(tokens)
    }
}

pub(crate) fn render_self_update(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = SelfUpdate::parse_tokens(tokens)?;
    let mode = if query.check.unwrap_or(false) {
        UpdateMode::Check
    } else {
        UpdateMode::Install
    };
    let updater = Updater::new(build_info::VERSION, build_info::TARGET)
        .map_err(|error| AppError::CommandExecutionError(error.to_string()))?;
    let outcome = updater
        .run(mode)
        .map_err(|error| AppError::CommandExecutionError(error.to_string()))?;
    let message = match outcome.status() {
        UpdateStatus::UpToDate => "No newer stable release is available.",
        UpdateStatus::Available => "A newer stable release is available. Run self-update to install it.",
        UpdateStatus::Updated => "Executable updated. Restart the CLI to use the new version; this process is still running the old version.",
    };
    render_json_record(
        tokens,
        &JsonRecord::from(json!({
            "status": outcome.status().as_str(),
            "current_version": build_info::VERSION,
            "release_version": format!("v{}", outcome.release_version()),
            "target": build_info::TARGET,
            "executable": outcome.executable(),
            "restart_required": outcome.status() == UpdateStatus::Updated,
            "message": message,
        })),
    )
}
