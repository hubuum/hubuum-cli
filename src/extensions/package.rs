use std::fs::read_to_string;
use std::path::Path;

use hubuum_extension_protocol::{ExtensionManifest, MANIFEST_FILENAME};
use semver::Version;

use crate::errors::AppError;

pub(crate) fn validate_package_source(source: &Path) -> Result<ExtensionManifest, AppError> {
    if !source.is_dir() {
        return Err(AppError::CommandExecutionError(format!(
            "extension source '{}' is not a directory",
            source.display()
        )));
    }
    let manifest_path = source.join(MANIFEST_FILENAME);
    let manifest = ExtensionManifest::parse(&read_to_string(&manifest_path)?).map_err(|error| {
        AppError::CommandExecutionError(format!(
            "invalid extension manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let cli_version = Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version");
    if !manifest.supports_cli(&cli_version) {
        return Err(AppError::CommandExecutionError(format!(
            "extension requires CLI {}, but this CLI is {cli_version}",
            manifest.requires_cli()
        )));
    }
    if let Some(executable) = manifest.executable() {
        validate_executable(&source.join(executable.as_path()))?;
    }
    Ok(manifest)
}

fn validate_executable(path: &Path) -> Result<(), AppError> {
    let metadata = path.metadata().map_err(|error| {
        AppError::CommandExecutionError(format!(
            "could not inspect extension executable '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(AppError::CommandExecutionError(format!(
            "extension executable '{}' is not a file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(AppError::CommandExecutionError(format!(
                "extension executable '{}' is not executable",
                path.display()
            )));
        }
    }
    Ok(())
}
