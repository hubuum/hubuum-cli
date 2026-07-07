use anstream::ColorChoice;
use anstyle::{Ansi256Color, AnsiColor, Color, Style};

use crate::models::OutputColor;

#[derive(Debug, Clone, Copy)]
pub enum ThemeRole {
    Error,
    Warning,
    Muted,
    Prompt,
    Heading,
    TableBand,
}

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

    let style = role_style(role);
    format!("{}{}{}", style.render(), text, style.render_reset())
}

fn role_style(role: ThemeRole) -> Style {
    match role {
        ThemeRole::Error => Style::new()
            .bold()
            .fg_color(Some(Color::Ansi(AnsiColor::Red))),
        ThemeRole::Warning => Style::new()
            .bold()
            .fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        ThemeRole::Muted => Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
        ThemeRole::Prompt => Style::new()
            .bold()
            .fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
        ThemeRole::Heading => Style::new()
            .bold()
            .fg_color(Some(Color::Ansi(AnsiColor::Green))),
        ThemeRole::TableBand => Style::new().bg_color(Some(Color::Ansi256(Ansi256Color(236)))),
    }
}
