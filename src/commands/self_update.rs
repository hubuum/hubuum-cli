use super::OfflineExecution;
use std::time::Duration;

use cli_command_derive::CommandArgs;
use hubuum_update::{UpdateError, UpdateMode, UpdateStatus, Updater};
use serde::Serialize;
use serde_json::json;

use super::builder::{catalog_command, CommandDocs};
use super::{render_json_record, CliCommand};
use crate::build_info;
use crate::catalog::CommandCatalogBuilder;
use crate::config::{get_config, SelfUpdateConfig};
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
                long_about: Some("Download the official release for this platform, verify its SHA-256 checksum, and replace the running executable on disk. No Hubuum login is required. Use --check to inspect availability without downloading or installing. The installation directory must be writable. Restart an active REPL after updating; the current process keeps running the old version. Package-managed installations should use their package manager; set self_update.enabled=false to disable both update checks and installation. Set self_update.check_on_startup=true to check at interactive shell startup; it never installs automatically. Rolling main builds and prereleases are not update destinations."),
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
    const OFFLINE: OfflineExecution = OfflineExecution::Always(render_self_update);

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_self_update(tokens)
    }
}

pub(crate) fn render_self_update(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let query = SelfUpdate::parse_tokens(tokens)?;
    if !get_config().self_update.enabled() {
        return Err(AppError::CommandExecutionError(
            "Self-update is disabled by configuration (self_update.enabled=false). Use your package manager or contact your administrator."
                .to_string(),
        ));
    }
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

pub(crate) fn startup_notice(config: &SelfUpdateConfig) -> Option<String> {
    startup_notice_with(config, || {
        let outcome = Updater::new(build_info::VERSION, build_info::TARGET)?
            .with_timeout(Duration::from_secs(3))
            .run(UpdateMode::Check)?;
        Ok((outcome.status() == UpdateStatus::Available)
            .then(|| outcome.release_version().to_string()))
    })
}

fn startup_notice_with(
    config: &SelfUpdateConfig,
    check: impl FnOnce() -> Result<Option<String>, UpdateError>,
) -> Option<String> {
    if !config.enabled() || !config.check_on_startup() {
        return None;
    }
    match check() {
        Ok(Some(version)) => Some(format!(
            "Hubuum CLI v{version} is available. Run self-update to install it."
        )),
        Ok(None) => None,
        Err(error) => {
            log::debug!("Startup update check failed: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn startup_check_requires_explicit_opt_in_and_enabled_updates() {
        let default = SelfUpdateConfig::default();
        assert!(default.enabled());
        assert!(!default.check_on_startup());
        for settings in [
            json!({}),
            json!({"enabled": true, "check_on_startup": false}),
            json!({"enabled": false, "check_on_startup": true}),
            json!({"enabled": false, "check_on_startup": false}),
        ] {
            let config = serde_json::from_value(settings).unwrap();
            assert!(startup_notice_with(&config, || panic!("must not contact GitHub")).is_none());
        }
    }

    #[test]
    fn startup_check_only_announces_available_updates() {
        let config = serde_json::from_value(json!({"check_on_startup": true})).unwrap();
        assert_eq!(
            startup_notice_with(&config, || Ok(Some("1.2.3".to_string()))).as_deref(),
            Some("Hubuum CLI v1.2.3 is available. Run self-update to install it.")
        );
        assert!(startup_notice_with(&config, || Ok(None)).is_none());
        assert!(
            startup_notice_with(&config, || Err(UpdateError::Update("offline".to_string())))
                .is_none()
        );
    }
}
