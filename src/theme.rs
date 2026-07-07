use std::path::{Path, PathBuf};

use anstream::ColorChoice;

use crate::models::OutputColor;

pub use hubuum_theme::ThemeRole;

pub fn color_choice() -> ColorChoice {
    match crate::config::get_config().output.color {
        OutputColor::Auto => ColorChoice::Auto,
        OutputColor::Always => ColorChoice::Always,
        OutputColor::Never => ColorChoice::Never,
    }
}

pub fn paint(role: ThemeRole, text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if matches!(crate::config::get_config().output.color, OutputColor::Never) {
        return text.to_string();
    }

    match active_theme() {
        Ok(theme) => hubuum_theme::paint(&theme, role, text),
        Err(_) => text.to_string(),
    }
}

pub fn paint_command(text: impl AsRef<str>) -> String {
    paint(ThemeRole::Command, text)
}

pub fn active_theme() -> Result<hubuum_theme::Theme, hubuum_theme::ThemeError> {
    let cfg = crate::config::get_config();
    resolve_theme(
        &cfg.output.theme,
        theme_file_path(&cfg.output.theme_file).as_deref(),
    )
}

pub fn available_themes() -> Result<hubuum_theme::ThemeCatalog, hubuum_theme::ThemeError> {
    let cfg = crate::config::get_config();
    hubuum_theme::catalog(theme_file_path(&cfg.output.theme_file).as_deref())
}

pub fn resolve_theme(
    name: &str,
    theme_file: Option<&Path>,
) -> Result<hubuum_theme::Theme, hubuum_theme::ThemeError> {
    match hubuum_theme::resolve_theme(name, theme_file) {
        Ok(theme) => Ok(theme),
        Err(_) => hubuum_theme::resolve_theme(hubuum_theme::DEFAULT_THEME, None),
    }
}

fn theme_file_path(path: &str) -> Option<PathBuf> {
    (!path.is_empty()).then(|| expand_home(path))
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}
