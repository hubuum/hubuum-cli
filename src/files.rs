use std::path::{Path, PathBuf};

use crate::{errors::AppError, models::TokenEntry};

fn ensure_root_dir() -> Result<PathBuf, AppError> {
    let root_dir = dirs::data_dir()
        .ok_or_else(|| AppError::DataDirError("Could not determine data directory".to_string()))?
        .join("hubuum_cli");

    if !root_dir.exists() {
        std::fs::create_dir_all(&root_dir)?;
    }

    Ok(root_dir)
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
    dirs::config_dir()
        .map(|mut path| {
            path.push(".hubuum_cli/config.toml");
            path
        })
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn ensure_file_exists(file: &str) -> Result<PathBuf, AppError> {
    let fqfile = ensure_root_dir()?.join(file);
    log::trace!("Checking file: {fqfile:?}");
    if !fqfile.exists() {
        log::debug!("Creating file: {fqfile:?}");
        if file == "token.json" {
            std::fs::write(fqfile.clone(), "[]")?;
        } else {
            std::fs::write(fqfile.clone(), "")?;
        }
    }
    Ok(fqfile)
}

pub fn get_history_file() -> Result<PathBuf, AppError> {
    ensure_file_exists("history.txt")
}

pub fn get_token_file() -> Result<PathBuf, AppError> {
    ensure_file_exists("token.json")
}

pub fn get_log_file() -> Result<PathBuf, AppError> {
    ensure_file_exists("log.txt")
}

pub fn get_token_from_tokenfile(
    hostname: &str,
    username: &str,
) -> Result<Option<String>, AppError> {
    let token_file_path = get_token_file()?;
    get_token_from_file(&token_file_path, hostname, username)
}

fn get_token_from_file(
    token_file_path: &Path,
    hostname: &str,
    username: &str,
) -> Result<Option<String>, AppError> {
    let token_entries = read_token_entries(token_file_path)?;
    for token_entry in &token_entries {
        if token_entry.hostname == hostname && token_entry.username == username {
            return Ok(Some(token_entry.token.clone()));
        }
    }
    Ok(None)
}

pub fn write_token_to_tokenfile(token_entry: TokenEntry) -> Result<(), AppError> {
    let token_file_path = get_token_file()?;
    write_token_to_file(&token_file_path, token_entry)
}

fn write_token_to_file(token_file_path: &Path, token_entry: TokenEntry) -> Result<(), AppError> {
    let mut token_entries = read_token_entries_or_default(token_file_path)?;

    token_entries.retain(|entry| {
        entry.hostname != token_entry.hostname || entry.username != token_entry.username
    });
    token_entries.push(token_entry);

    write_token_entries(token_file_path, &token_entries)?;

    Ok(())
}

fn read_token_entries(token_file_path: &Path) -> Result<Vec<TokenEntry>, AppError> {
    let token_file_content = std::fs::read_to_string(token_file_path)?;
    Ok(serde_json::from_str(&token_file_content)?)
}

fn read_token_entries_or_default(token_file_path: &Path) -> Result<Vec<TokenEntry>, AppError> {
    match std::fs::read_to_string(token_file_path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(_) => Ok(Vec::new()),
    }
}

fn write_token_entries(token_file_path: &Path, entries: &[TokenEntry]) -> Result<(), AppError> {
    let token_file_content = serde_json::to_string(entries)?;
    std::fs::write(token_file_path, token_file_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::errors::AppError;
    use crate::models::TokenEntry;
    use rstest::{fixture, rstest};
    use tempfile::NamedTempFile;

    use super::{get_token_from_file, write_token_to_file};

    #[fixture]
    fn token_file() -> NamedTempFile {
        NamedTempFile::new().expect("temp token file should be created")
    }

    #[rstest]
    fn write_overwrites_existing_token_for_same_host_and_user(token_file: NamedTempFile) {
        let path = token_file.path();
        let initial = vec![TokenEntry {
            hostname: "h1".to_string(),
            username: "u1".to_string(),
            token: "old".to_string(),
        }];
        fs::write(
            &path,
            serde_json::to_string(&initial).expect("serialize initial tokens"),
        )
        .expect("write initial token file");

        write_token_to_file(
            &path,
            TokenEntry {
                hostname: "h1".to_string(),
                username: "u1".to_string(),
                token: "new".to_string(),
            },
        )
        .expect("write should succeed");

        let token = get_token_from_file(path, "h1", "u1").expect("token lookup should succeed");
        assert_eq!(token, Some("new".to_string()));
    }

    #[rstest]
    fn write_preserves_other_entries(token_file: NamedTempFile) {
        let path = token_file.path();
        let initial = vec![
            TokenEntry {
                hostname: "h1".to_string(),
                username: "u1".to_string(),
                token: "old".to_string(),
            },
            TokenEntry {
                hostname: "h2".to_string(),
                username: "u2".to_string(),
                token: "other".to_string(),
            },
        ];
        fs::write(
            &path,
            serde_json::to_string(&initial).expect("serialize initial tokens"),
        )
        .expect("write initial token file");

        write_token_to_file(
            &path,
            TokenEntry {
                hostname: "h1".to_string(),
                username: "u1".to_string(),
                token: "new".to_string(),
            },
        )
        .expect("write should succeed");

        let token = get_token_from_file(path, "h2", "u2").expect("token lookup should succeed");
        assert_eq!(token, Some("other".to_string()));
    }

    #[rstest]
    fn invalid_json_returns_parse_error(token_file: NamedTempFile) {
        let path = token_file.path();
        fs::write(&path, "{ invalid json").expect("write malformed token file");

        let err = get_token_from_file(path, "h1", "u1").expect_err("should fail on invalid json");
        assert!(matches!(err, AppError::ParseJsonError(_)));
    }
}
