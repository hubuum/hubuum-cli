use anstyle::{Color as AnsiStyleColor, Effects, Style};
use comfy_table::{Attribute, Cell, Color, Table};
use crossterm::style::force_color_output;
use hubuum_theme::{paint_style, Theme, ThemeRole};

use crate::theme::output_theme;

/// A single theme resolution shared by all cells in a rendering operation.
#[derive(Default)]
pub(super) struct OutputStyling {
    theme: Option<Theme>,
}

impl OutputStyling {
    pub(super) fn active() -> Self {
        Self {
            theme: output_theme(),
        }
    }

    pub(super) fn preview(theme: &Theme) -> Self {
        Self {
            theme: Some(theme.clone()),
        }
    }

    pub(super) fn style(&self, role: ThemeRole) -> Style {
        self.theme
            .as_ref()
            .map(|theme| theme.style(role))
            .unwrap_or_default()
    }

    pub(super) fn value_style(&self, text: &str) -> Style {
        self.theme
            .as_ref()
            .map(|theme| theme.value_style(text))
            .unwrap_or_default()
    }

    pub(super) fn paint(&self, role: ThemeRole, text: impl AsRef<str>) -> String {
        paint_style(self.style(role), text)
    }

    pub(super) fn value(&self, text: &str) -> String {
        paint_style(self.value_style(text), text)
    }

    pub(super) fn cell(&self, text: &str, style: Style) -> Cell {
        let cell = Cell::new(text);
        if self.theme.is_none() {
            return cell;
        }
        // Native cell styles are applied after comfy-table measures and wraps text.
        // Explicit resets keep the surrounding border style out of ordinary cells.
        cell.fg(table_color(style.get_fg_color()))
            .bg(table_color(style.get_bg_color()))
            .add_attribute(if style.get_effects().contains(Effects::BOLD) {
                Attribute::Bold
            } else {
                Attribute::NormalIntensity
            })
    }

    pub(super) fn table(&self, table: &mut Table) -> String {
        if self.theme.is_some() {
            // The output sink has already resolved auto/always/never. In particular,
            // --color always must override crossterm's independent NO_COLOR check.
            force_color_output(true);
            table.enforce_styling();
        }
        let text = table.to_string();
        let border = self.style(ThemeRole::Border);
        // Cell resets restore the surrounding border, including vertical separators.
        // This styles structure without interpreting border-like characters in data.
        let text = text.replace("\x1b[0m", &format!("\x1b[0m{}", border.render()));
        paint_style(border, text)
    }
}

fn table_color(color: Option<AnsiStyleColor>) -> Color {
    match color {
        None => Color::Reset,
        Some(AnsiStyleColor::Ansi(color)) => Color::AnsiValue(color as u8),
        Some(AnsiStyleColor::Ansi256(color)) => Color::AnsiValue(color.0),
        Some(AnsiStyleColor::Rgb(color)) => Color::Rgb {
            r: color.0,
            g: color.1,
            b: color.2,
        },
    }
}
