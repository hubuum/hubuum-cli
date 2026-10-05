//! Install verified, stable releases of the Hubuum CLI without server credentials.

use std::env::current_exe;
use std::path::{Path, PathBuf};
use std::time::Duration;

use self_update::backends::github::{Update, UpdateBuilder};
use self_update::Release;
use semver::Version;
use thiserror::Error;

const API_URL: &str = "https://api.github.com";

/// Errors are independent of both application configuration and updater internals.
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Invalid running version: {0}")]
    InvalidVersion(String),
    #[error("No official Hubuum CLI binary is published for target '{0}'; update using your original installation method")]
    UnsupportedTarget(String),
    #[error("Unable to locate the running executable: {0}")]
    Executable(String),
    #[error("Self-update failed: {0}")]
    Update(String),
    #[error("The latest release is not a stable version: {0}")]
    UnstableRelease(String),
    #[error("Release v{version} is missing the required asset '{asset}'; the installed binary was not changed")]
    MissingAsset { version: String, asset: String },
}

/// Checking never downloads an archive or changes the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Check,
    Install,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Available,
    Updated,
}

impl UpdateStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpToDate => "up_to_date",
            Self::Available => "update_available",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug)]
pub struct UpdateOutcome {
    status: UpdateStatus,
    release_version: String,
    executable: PathBuf,
}

impl UpdateOutcome {
    pub fn status(&self) -> UpdateStatus {
        self.status
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

#[derive(Debug, Clone, Copy)]
enum Platform {
    LinuxX86_64,
    LinuxAarch64,
    MacOsAarch64,
    WindowsX86_64,
}

impl Platform {
    fn from_target(target: &str) -> Result<Self, UpdateError> {
        match target {
            "x86_64-unknown-linux-musl" | "x86_64-unknown-linux-gnu" => Ok(Self::LinuxX86_64),
            "aarch64-unknown-linux-musl" | "aarch64-unknown-linux-gnu" => Ok(Self::LinuxAarch64),
            "aarch64-apple-darwin" => Ok(Self::MacOsAarch64),
            "x86_64-pc-windows-msvc" => Ok(Self::WindowsX86_64),
            _ => Err(UpdateError::UnsupportedTarget(target.to_string())),
        }
    }

    fn asset_name(self, version: &Version) -> String {
        let (platform, extension) = match self {
            Self::LinuxX86_64 => ("linux-x86_64-musl", "tar.gz"),
            Self::LinuxAarch64 => ("linux-aarch64-musl", "tar.gz"),
            Self::MacOsAarch64 => ("macos-aarch64", "tar.gz"),
            Self::WindowsX86_64 => ("windows-x86_64", "zip"),
        };
        format!("hubuum-cli-{platform}-v{version}.{extension}")
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "hubuum-cli.exe",
            _ => "hubuum-cli",
        }
    }
}

/// A validated running build. Only official stable Hubuum CLI releases are used.
#[derive(Debug)]
pub struct Updater {
    current_version: Version,
    target: String,
    platform: Platform,
}

impl Updater {
    pub fn new(current_version: &str, target: &str) -> Result<Self, UpdateError> {
        let current_version =
            Version::parse(current_version.strip_prefix('v').unwrap_or(current_version))
                .map_err(|error| UpdateError::InvalidVersion(error.to_string()))?;
        Ok(Self {
            current_version,
            target: target.to_string(),
            platform: Platform::from_target(target)?,
        })
    }

    pub fn run(&self, mode: UpdateMode) -> Result<UpdateOutcome, UpdateError> {
        let executable =
            current_exe().map_err(|error| UpdateError::Executable(error.to_string()))?;
        self.run_at(mode, API_URL, executable)
    }

    fn builder(&self, api_url: &str, executable: &Path) -> UpdateBuilder {
        let mut builder = Update::configure();
        builder
            .repo_owner("hubuum")
            .repo_name("hubuum-cli")
            .api_base_url(api_url)
            .current_version(self.current_version.to_string())
            .target(&self.target)
            .bin_name(self.platform.binary_name())
            .bin_path_in_archive(self.platform.binary_name())
            .bin_install_path(executable)
            .timeout(Duration::from_secs(120))
            .check_install_path_writable(true)
            .show_output(false)
            .no_confirm(true);
        if api_url == API_URL {
            builder.auth_token_from_env();
        }
        builder
    }

    fn run_at(
        &self,
        mode: UpdateMode,
        api_url: &str,
        executable: PathBuf,
    ) -> Result<UpdateOutcome, UpdateError> {
        let mut builder = self.builder(api_url, &executable);
        let releases = builder
            .build()
            .map_err(updater_error)?
            .get_latest_release()
            .map_err(updater_error)?;
        let release = releases
            .latest()
            .ok_or_else(|| UpdateError::Update("No stable release was found".to_string()))?;
        let version = Version::parse(release.version())
            .map_err(|error| UpdateError::Update(error.to_string()))?;
        if !version.pre.is_empty() || !version.build.is_empty() {
            return Err(UpdateError::UnstableRelease(version.to_string()));
        }
        let mut outcome = UpdateOutcome {
            status: UpdateStatus::UpToDate,
            release_version: version.to_string(),
            executable,
        };
        // Build metadata does not make a rolling build newer or older than its
        // stable counterpart. Never downgrade a locally newer development build.
        if !version.cmp_precedence(&self.current_version).is_gt() {
            return Ok(outcome);
        }
        let asset_name = self.platform.asset_name(&version);
        let checksum_name = format!("{asset_name}.sha256");
        require_asset(release, &asset_name)?;
        require_asset(release, &checksum_name)?;
        outcome.status = UpdateStatus::Available;
        if mode == UpdateMode::Install {
            // Pin the selected version and exact archive; never let substring
            // matching choose a checksum, another platform, or a rolling asset.
            builder
                .release_tag(format!("v{version}"))
                .asset_matcher(move |assets| {
                    assets
                        .iter()
                        .find(|asset| asset.name() == asset_name)
                        .cloned()
                })
                .checksum_from_asset(checksum_name);
            builder
                .build()
                .map_err(updater_error)?
                .update()
                .map_err(updater_error)?;
            outcome.status = UpdateStatus::Updated;
        }
        Ok(outcome)
    }
}

fn require_asset(release: &Release, name: &str) -> Result<(), UpdateError> {
    if release.assets().iter().any(|asset| asset.name() == name) {
        Ok(())
    } else {
        Err(UpdateError::MissingAsset {
            version: release.version().to_string(),
            asset: name.to_string(),
        })
    }
}

fn updater_error(error: self_update::Error) -> UpdateError {
    UpdateError::Update(error.to_string())
}

#[cfg(test)]
mod tests;
