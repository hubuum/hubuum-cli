use super::OfflineExecution;
use cli_command_derive::CommandArgs;
use hubuum_filter::OutputEnvelope;
use hubuum_theme::{paint, paint_style, ThemeError};
use serde::Serialize;
use serde_json::json;

use super::builder::{catalog_command, CommandDocs};
use super::{desired_format, option_or_pos, response_format, CliCommand};
use crate::autocomplete::theme_names;
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::config::{
    get_config, reload_runtime_config, set_persisted_value, theme_value_candidates,
};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::models::OutputFormat;
use crate::output::{
    append_json_line, append_line, append_lines, render_dense_theme_preview, set_semantic_output,
};
use crate::services::AppServices;
use crate::theme::{available_themes, paint_command, ThemeRole};
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
    const OFFLINE: OfflineExecution = OfflineExecution::Always(render_theme_list);

    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_list(tokens)
    }
}

pub(crate) fn render_theme_list(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let _query = ThemeList::parse_tokens(tokens)?;
    let cfg = get_config();
    let rows = available_themes()
        .map_err(theme_error)?
        .themes()
        .map(|theme| {
            json!({
                "name": theme.name,
                "display_name": theme.display_name,
                "active": theme.name == cfg.output.theme,
                "license": theme.license.name,
                "source": theme.license.source.as_deref().unwrap_or(""),
            })
        })
        .collect::<Vec<_>>();
    match desired_format(tokens)? {
        OutputFormat::Json | OutputFormat::Text => {
            set_semantic_output(OutputEnvelope::rows(
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
    const OFFLINE: OfflineExecution = OfflineExecution::Always(render_theme_show);

    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_show(tokens)
    }
}

pub(crate) fn render_theme_show(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut query = ThemeShow::parse_tokens(tokens)?;
    query.name = option_or_pos(query.name, tokens, 0, "name")?;
    let name = query
        .name
        .unwrap_or_else(|| get_config().output.theme.clone());
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
    const OFFLINE: OfflineExecution = OfflineExecution::Always(render_theme_preview);

    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, _services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        render_theme_preview(tokens)
    }
}

pub(crate) fn render_theme_preview(tokens: &CommandTokenizer) -> Result<(), AppError> {
    let mut query = ThemePreview::parse_tokens(tokens)?;
    query.name = option_or_pos(query.name, tokens, 0, "name")?;
    let name = query
        .name
        .unwrap_or_else(|| get_config().output.theme.clone());
    let theme = available_themes()
        .map_err(theme_error)?
        .get(&name)
        .cloned()
        .ok_or_else(|| unknown_theme(&name))?;

    if response_format(tokens)? == OutputFormat::Json {
        return append_json_line(&theme);
    }

    append_line(paint(
        &theme,
        ThemeRole::Title,
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
        paint(&theme, ThemeRole::Heading, "Heading")
    ))?;
    append_line(format!("  {}", paint(&theme, ThemeRole::Prompt, "Prompt")))?;
    append_line(format!(
        "  {}",
        paint(
            &theme,
            ThemeRole::Command,
            "object list --class Hosts | P Name os_version"
        )
    ))?;
    append_line(format!(
        "  {}",
        paint(&theme, ThemeRole::Warning, "Warning: example warning")
    ))?;
    append_line(format!(
        "  {}",
        paint(&theme, ThemeRole::Error, "Error: example error")
    ))?;
    append_line(format!(
        "  {}",
        paint(&theme, ThemeRole::Muted, "Muted status text")
    ))?;
    for (role, label) in [
        (ThemeRole::Key, "Detail key"),
        (ThemeRole::TableHeader, "Table header"),
        (ThemeRole::Title, "Title"),
        (ThemeRole::Border, "Borders: | --- |"),
        (ThemeRole::Message, "Message: example confirmation"),
    ] {
        append_line(format!("  {}", paint(&theme, role, label)))?;
    }
    append_line("")?;
    append_line(paint(&theme, ThemeRole::Title, "Value classifications"))?;
    for value in [
        "42",
        "192.168.0.1",
        "2001:db8::1",
        "true",
        "false",
        "null",
        "hubuum-cli object list",
        "192.168.0.0/24",
        "2026-10-26",
        "2020-07-30T11:46:02",
        "https://example.org",
        "00:1a:2b:3c:4d:5e",
        "ordinary text",
    ] {
        append_line(format!(
            "  {:<14} {}",
            format!("{:?}", theme.classify(value)),
            paint_style(theme.value_style(value), value)
        ))?;
    }
    append_line("")?;
    append_line(paint(
        &theme,
        ThemeRole::Title,
        "Dense table with alternating row bands",
    ))?;
    append_lines(&render_dense_theme_preview(&theme))?;
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
        let catalog = available_themes().map_err(theme_error)?;
        if catalog.get(&name).is_none() {
            return Err(unknown_theme(&name));
        }

        let path = set_persisted_value("output.theme", &name)?;
        reload_runtime_config()?;
        services.invalidate_completion();
        services.sync_user_preferences_if_enabled()?;

        match response_format(tokens)? {
            OutputFormat::Json => append_json_line(&json!({
                "theme": name,
                "path": path,
                "note": "Saved and reloaded for this CLI session."
            }))?,
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
    let theme = available_themes()
        .map_err(theme_error)?
        .get(name)
        .cloned()
        .ok_or_else(|| unknown_theme(name))?;
    let rows = ThemeRole::ALL
        .into_iter()
        .map(|role| {
            let style = theme.role_style(role);
            json!({
                "role": format!("{role:?}"),
                "foreground": style.fg.map(|color| color.to_string()).unwrap_or_default(),
                "background": style.bg.map(|color| color.to_string()).unwrap_or_default(),
                "bold": style.bold,
                "dimmed": style.dimmed,
            })
        })
        .collect::<Vec<_>>();
    let detail = json!({
        "name": theme.name,
        "display_name": theme.display_name,
        "license": theme.license.name,
        "source": theme.license.source.as_deref().unwrap_or_default(),
        "roles": rows,
        "classifiers": theme.classifiers(),
    });
    set_semantic_output(OutputEnvelope::detail(
        detail,
        vec![
            "name".to_string(),
            "display_name".to_string(),
            "license".to_string(),
            "source".to_string(),
            "roles".to_string(),
            "classifiers".to_string(),
        ],
    ))
}

fn unknown_theme(name: &str) -> AppError {
    let names = theme_value_candidates().join(", ");
    AppError::ConfigError(format!("Unknown theme: {name}. Use one of: {names}"))
}

fn theme_error(err: ThemeError) -> AppError {
    AppError::ConfigError(err.to_string())
}
