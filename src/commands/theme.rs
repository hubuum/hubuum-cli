use cli_command_derive::CommandArgs;
use serde::Serialize;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, option_or_pos, CliCommand};
use crate::autocomplete::theme_names;
use crate::catalog::CommandCatalogBuilder;
use crate::config::{reload_runtime_config, set_persisted_value};
use crate::errors::AppError;
use crate::models::OutputFormat;
use crate::output::{append_line, set_semantic_output};
use crate::services::AppServices;
use crate::theme::{paint, paint_command, ThemeRole};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    builder
        .add_command(
            &["theme"],
            catalog_command(
                "list",
                ThemeList::default(),
                CommandDocs {
                    about: Some("List available color themes"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["theme"],
            catalog_command(
                "show",
                ThemeShow::default(),
                CommandDocs {
                    about: Some("Show a color theme"),
                    examples: Some("catppuccin-mocha"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["theme"],
            catalog_command(
                "preview",
                ThemePreview::default(),
                CommandDocs {
                    about: Some("Preview a color theme"),
                    examples: Some("solarized-dark"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["theme"],
            catalog_command(
                "use",
                ThemeUse::default(),
                CommandDocs {
                    about: Some("Persist and activate a color theme"),
                    examples: Some("catppuccin-mocha"),
                    ..CommandDocs::default()
                },
            ),
        );
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ThemeList {}

impl CliCommand for ThemeList {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_list(tokens)
    }
}

pub(crate) fn render_theme_list(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let _query = ThemeList::parse_tokens(tokens)?;
    let cfg = crate::config::get_config();
    let rows = crate::theme::available_themes()
        .map_err(theme_error)?
        .themes()
        .map(|theme| {
            serde_json::json!({
                "name": theme.name,
                "display_name": theme.display_name,
                "active": theme.name == cfg.output.theme,
                "license": theme.license.name,
                "source": theme.license.source.as_deref().unwrap_or(""),
            })
        })
        .collect::<Vec<_>>();
    match desired_format(tokens) {
        OutputFormat::Json | OutputFormat::Text => {
            set_semantic_output(hubuum_filter::OutputEnvelope::rows(
                rows,
                vec![
                    "name".to_string(),
                    "display_name".to_string(),
                    "active".to_string(),
                    "license".to_string(),
                    "source".to_string(),
                ],
            ))?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ThemeShow {
    #[option(
        short = "n",
        long = "name",
        help = "Theme name",
        autocomplete = "theme_names"
    )]
    pub name: Option<String>,
}

impl CliCommand for ThemeShow {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_show(tokens)
    }
}

pub(crate) fn render_theme_show(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut query = ThemeShow::parse_tokens(tokens)?;
    query.name = option_or_pos(query.name, tokens, 0, "name")?;
    let name = query
        .name
        .unwrap_or_else(|| crate::config::get_config().output.theme);
    render_theme_detail(&name)
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ThemePreview {
    #[option(
        short = "n",
        long = "name",
        help = "Theme name",
        autocomplete = "theme_names"
    )]
    pub name: Option<String>,
}

impl CliCommand for ThemePreview {
    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_preview(tokens)
    }
}

pub(crate) fn render_theme_preview(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut query = ThemePreview::parse_tokens(tokens)?;
    query.name = option_or_pos(query.name, tokens, 0, "name")?;
    let name = query
        .name
        .unwrap_or_else(|| crate::config::get_config().output.theme);
    let theme = crate::theme::available_themes()
        .map_err(theme_error)?
        .get(&name)
        .cloned()
        .ok_or_else(|| unknown_theme(&name))?;

    append_line(paint(
        ThemeRole::Heading,
        format!("Theme: {}", theme.display_name),
    ))?;
    append_line(format!("  Name: {}", theme.name))?;
    append_line(format!("  License: {}", theme.license.name))?;
    if let Some(source) = &theme.license.source {
        append_line(format!("  Source: {source}"))?;
    }
    append_line("")?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(&theme, ThemeRole::Heading, "Heading")
    ))?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(&theme, ThemeRole::Prompt, "Prompt")
    ))?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(
            &theme,
            ThemeRole::Command,
            "object list --class Hosts | P Name os_version"
        )
    ))?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(&theme, ThemeRole::Warning, "Warning: example warning")
    ))?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(&theme, ThemeRole::Error, "Error: example error")
    ))?;
    append_line(format!(
        "  {}",
        hubuum_theme::paint(&theme, ThemeRole::Muted, "Muted status text")
    ))?;
    append_line(hubuum_theme::paint(
        &theme,
        ThemeRole::TableBand,
        "  Name              os_version",
    ))?;
    Ok(())
}

#[derive(Debug, Serialize, Clone, CommandArgs, Default)]
pub struct ThemeUse {
    #[option(
        short = "n",
        long = "name",
        help = "Theme name",
        autocomplete = "theme_names"
    )]
    pub name: Option<String>,
}

impl CliCommand for ThemeUse {
    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.name = option_or_pos(query.name, tokens, 0, "name")?;
        let name = query
            .name
            .ok_or_else(|| AppError::ParseError("Theme name is required".to_string()))?;
        let catalog = crate::theme::available_themes().map_err(theme_error)?;
        if catalog.get(&name).is_none() {
            return Err(unknown_theme(&name));
        }

        let path = set_persisted_value("output.theme", &name)?;
        reload_runtime_config()?;
        services.invalidate_completion();

        match desired_format(tokens) {
            OutputFormat::Json => {
                append_line(&serde_json::to_string_pretty(&serde_json::json!({
                    "theme": name,
                    "path": path,
                    "note": "Saved and reloaded for this CLI session."
                }))?)?
            }
            OutputFormat::Text => append_line(format!(
                "Saved {} to {} and reloaded the current session.",
                paint_command(&name),
                path.display()
            ))?,
        }
        Ok(())
    }
}

fn render_theme_detail(name: &str) -> Result<(), AppError> {
    let theme = crate::theme::available_themes()
        .map_err(theme_error)?
        .get(name)
        .cloned()
        .ok_or_else(|| unknown_theme(name))?;
    let rows = theme
        .roles
        .iter()
        .map(|(role, style)| {
            serde_json::json!({
                "role": format!("{role:?}"),
                "foreground": style.fg.map(|color| color.to_string()).unwrap_or_default(),
                "background": style.bg.map(|color| color.to_string()).unwrap_or_default(),
                "bold": style.bold,
            })
        })
        .collect::<Vec<_>>();
    let detail = serde_json::json!({
        "name": theme.name,
        "display_name": theme.display_name,
        "license": theme.license.name,
        "source": theme.license.source.unwrap_or_default(),
        "roles": rows,
    });
    set_semantic_output(hubuum_filter::OutputEnvelope::detail(
        detail,
        vec![
            "name".to_string(),
            "display_name".to_string(),
            "license".to_string(),
            "source".to_string(),
            "roles".to_string(),
        ],
    ))
}

fn unknown_theme(name: &str) -> AppError {
    let names = crate::config::theme_value_candidates().join(", ");
    AppError::ConfigError(format!("Unknown theme: {name}. Use one of: {names}"))
}

fn theme_error(err: hubuum_theme::ThemeError) -> AppError {
    AppError::ConfigError(err.to_string())
}
