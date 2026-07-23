use std::fs::{create_dir_all, read_to_string, File, OpenOptions};
use std::io::{Error, ErrorKind, Write};
use std::path::{Path, PathBuf};

use dirs::{config_dir, data_dir};
use log::{debug, trace};
use serde_json::{from_str, to_string};

use crate::{errors::AppError, models::TokenEntry};

#[derive(Clone, Copy)]
enum DataFile {
    History,
    Log,
    Token,
}

impl DataFile {
    fn name(self) -> &'static str {
        match self {
            Self::History => "history.txt",
            Self::Log => "log.txt",
            Self::Token => "token.json",
        }
    }

    fn initial_contents(self) -> &'static str {
        match self {
            Self::Token => "[]",
            Self::History | Self::Log => "",
        }
    }
}

fn data_root_dir() -> Result<PathBuf, AppError> {
    Ok(data_dir()
        .ok_or_else(|| AppError::DataDirError("Could not determine data directory".to_string()))?
        .join("hubuum_cli"))
}

fn ensure_root_dir_at(root_dir: &Path) -> Result<(), AppError> {
    create_dir_all(root_dir)?;
    set_owner_only_directory_permissions(root_dir)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_directory_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_directory_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

pub fn get_system_config_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\hubuum_cli\config.toml")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/hubuum_cli/config.toml")
    } else {
        PathBuf::from("/etc/hubuum_cli/config.toml")
    }
}

pub fn get_user_config_path() -> PathBuf {
    config_dir()
        .map(|mut path| {
            path.push(".hubuum_cli/config.toml");
            path
        })
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn ensure_file_exists(file: DataFile) -> Result<PathBuf, AppError> {
    let root_dir = data_root_dir()?;
    ensure_file_exists_at(&root_dir, file)
}

fn ensure_file_exists_at(root_dir: &Path, file: DataFile) -> Result<PathBuf, AppError> {
    ensure_root_dir_at(root_dir)?;
    let fqfile = root_dir.join(file.name());
    trace!("Checking file: {fqfile:?}");

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }

    match options.open(&fqfile) {
        Ok(mut handle) => {
            debug!("Creating file: {fqfile:?}");
            handle.write_all(file.initial_contents().as_bytes())?;
            handle.sync_all()?;
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }

    if !fqfile.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("Managed data path is not a file: {}", fqfile.display()),
        )
        .into());
    }

    set_owner_only_file_permissions(&fqfile)?;
    Ok(fqfile)
}

pub fn get_history_file() -> Result<PathBuf, AppError> {
    ensure_file_exists(DataFile::History)
}

pub fn get_token_file() -> Result<PathBuf, AppError> {
    ensure_file_exists(DataFile::Token)
}

pub fn get_log_file() -> Result<PathBuf, AppError> {
    ensure_file_exists(DataFile::Log)
}

pub fn get_token_from_tokenfile(
    hostname: &str,
    identity_scope: Option<&str>,
    username: &str,
) -> Result<Option<String>, AppError> {
    let token_file_path = get_token_file()?;
    let token_file_content = read_to_string(token_file_path)?;
    let token_entries: Vec<TokenEntry> = from_str(&token_file_content)?;

    for token_entry in &token_entries {
        if token_entry.hostname == hostname
            && token_entry.identity_scope.as_deref() == identity_scope
            && token_entry.username == username
        {
            return Ok(Some(token_entry.token.clone()));
        }
    }
    Ok(None)
}

pub fn write_token_to_tokenfile(token_entry: TokenEntry) -> Result<(), AppError> {
    let token_file_path = get_token_file()?;
    let token_file_content = read_to_string(&token_file_path)?;
    let mut token_entries: Vec<TokenEntry> = from_str(&token_file_content)?;

    token_entries.retain(|entry| {
        entry.hostname != token_entry.hostname
            || entry.identity_scope != token_entry.identity_scope
            || entry.username != token_entry.username
    });
    token_entries.push(token_entry);

    let token_file_content = to_string(&token_entries)?;
    let mut token_file = File::options()
        .write(true)
        .truncate(true)
        .open(token_file_path)?;
    token_file.write_all(token_file_content.as_bytes())?;
    token_file.sync_all()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{read_to_string, write};

    use tempfile::tempdir;

    use super::{ensure_file_exists_at, DataFile};

    #[test]
    fn token_file_starts_with_an_empty_json_array() {
        let directory = tempdir().expect("temporary directory should be created");

        let path = ensure_file_exists_at(directory.path(), DataFile::Token)
            .expect("token file should be created");

        assert_eq!(
            read_to_string(path).expect("token file should be readable"),
            "[]"
        );
    }

    #[test]
    fn existing_managed_files_are_not_overwritten() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join(DataFile::History.name());
        write(&path, "existing history").expect("history fixture should be written");

        ensure_file_exists_at(directory.path(), DataFile::History)
            .expect("existing history file should be accepted");

        assert_eq!(
            read_to_string(path).expect("history file should be readable"),
            "existing history"
        );
    }

    #[cfg(unix)]
    #[test]
    fn data_directory_and_managed_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("temporary directory should be created");
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
            .expect("directory permissions should be widened for the fixture");

        let paths = [DataFile::History, DataFile::Log, DataFile::Token].map(|file| {
            let path = directory.path().join(file.name());
            write(&path, file.initial_contents()).expect("fixture should be written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("file permissions should be widened for the fixture");
            ensure_file_exists_at(directory.path(), file).expect("managed file should be secured")
        });

        let directory_mode = directory
            .path()
            .metadata()
            .expect("directory metadata should be available")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);

        for path in paths {
            let file_mode = path
                .metadata()
                .expect("file metadata should be available")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
        }
    }
}
