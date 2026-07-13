use std::fs::{create_dir_all, read_to_string, write};
use std::path::PathBuf;

use dirs::{config_dir, data_dir};
use log::{debug, trace};
use serde_json::{from_str, to_string};

use crate::{errors::AppError, models::TokenEntry};

fn ensure_root_dir() -> Result<PathBuf, AppError> {
    let root_dir = data_dir()
        .ok_or_else(|| AppError::DataDirError("Could not determine data directory".to_string()))?
        .join("hubuum_cli");

    if !root_dir.exists() {
        create_dir_all(&root_dir)?;
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
    config_dir()
        .map(|mut path| {
            path.push(".hubuum_cli/config.toml");
            path
        })
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn ensure_file_exists(file: &str) -> Result<PathBuf, AppError> {
    let fqfile = ensure_root_dir()?.join(file);
    trace!("Checking file: {fqfile:?}");
    if !fqfile.exists() {
        debug!("Creating file: {fqfile:?}");
        if file == "token.json" {
            write(fqfile.clone(), "[]")?;
        } else {
            write(fqfile.clone(), "")?;
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
    let token_file_content = read_to_string(token_file_path)?;
    let token_entries: Vec<TokenEntry> = from_str(&token_file_content)?;

    for token_entry in &token_entries {
        if token_entry.hostname == hostname && token_entry.username == username {
            return Ok(Some(token_entry.token.clone()));
        }
    }
    Ok(None)
}

pub fn write_token_to_tokenfile(token_entry: TokenEntry) -> Result<(), AppError> {
    let token_file_path = get_token_file()?;
    let mut token_entries: Vec<TokenEntry> = match read_to_string(&token_file_path) {
        Ok(content) => from_str(&content)?,
        Err(_) => Vec::new(),
    };

    token_entries.retain(|entry| {
        entry.hostname != token_entry.hostname || entry.username != token_entry.username
    });
    token_entries.push(token_entry);

    let token_file_content = to_string(&token_entries)?;
    write(token_file_path, token_file_content)?;

    Ok(())
}
