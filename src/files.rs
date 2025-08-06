use std::path::PathBuf;

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

fn ensure_file_exists(file: &str) -> Result<PathBuf, AppError> {
    let fqfile = ensure_root_dir()?.join(file);
    log::trace!("Checking file: {:?}", fqfile);
    if !fqfile.exists() {
        log::debug!("Creating file: {:?}", fqfile);
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
    let token_file_content = std::fs::read_to_string(token_file_path)?;
    let token_entries: Vec<TokenEntry> = serde_json::from_str(&token_file_content)?;

    for token_entry in &token_entries {
        if token_entry.hostname == hostname && token_entry.username == username {
            return Ok(Some(token_entry.token.clone()));
        }
    }
    Ok(None)
}

pub fn write_token_to_tokenfile(token_entry: TokenEntry) -> Result<(), AppError> {
    let token_file_path = get_token_file()?;
    let mut token_entries: Vec<TokenEntry> = match std::fs::read_to_string(&token_file_path) {
        Ok(content) => serde_json::from_str(&content)?,
        Err(_) => Vec::new(),
    };

    token_entries.retain(|entry| {
        entry.hostname != token_entry.hostname || entry.username != token_entry.username
    });
    token_entries.push(token_entry);

    let token_file_content = serde_json::to_string(&token_entries)?;
    std::fs::write(token_file_path, token_file_content)?;

    Ok(())
}
