use std::path::{Path, PathBuf};

use anstream::ColorChoice;
use dirs::home_dir;
use hubuum_theme::{
    catalog as theme_catalog, paint as paint_theme, resolve_theme as resolve_hubuum_theme, Theme,
    ThemeCatalog, ThemeError, DEFAULT_THEME,
};

use crate::config::get_config;
use crate::models::OutputColor;

pub use hubuum_theme::ThemeRole;

pub fn color_choice() -> ColorChoice {
    match get_config().output.color {
        OutputColor::Auto => ColorChoice::Auto,
        OutputColor::Always => ColorChoice::Always,
        OutputColor::Never => ColorChoice::Never,
    }
}

pub fn paint(role: ThemeRole, text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if matches!(get_config().output.color, OutputColor::Never) {
        return text.to_string();
    }

    match active_theme() {
        Ok(theme) => paint_theme(&theme, role, text),
        Err(_) => text.to_string(),
    }
}

pub fn paint_command(text: impl AsRef<str>) -> String {
    paint(ThemeRole::Command, text)
}

pub fn active_theme() -> Result<Theme, ThemeError> {
    let cfg = get_config();
    resolve_theme(
        &cfg.output.theme,
        theme_file_path(&cfg.output.theme_file).as_deref(),
    )
}

pub fn available_themes() -> Result<ThemeCatalog, ThemeError> {
    let cfg = get_config();
    theme_catalog(theme_file_path(&cfg.output.theme_file).as_deref())
}

pub fn resolve_theme(name: &str, theme_file: Option<&Path>) -> Result<Theme, ThemeError> {
    match resolve_hubuum_theme(name, theme_file) {
        Ok(theme) => Ok(theme),
        Err(_) => resolve_hubuum_theme(DEFAULT_THEME, None),
    }
}

fn theme_file_path(path: &str) -> Option<PathBuf> {
    (!path.is_empty()).then(|| expand_home(path))
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}
