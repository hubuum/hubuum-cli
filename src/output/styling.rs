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
        let mut cell = cell
            .fg(table_color(style.get_fg_color()))
            .bg(table_color(style.get_bg_color()));
        let effects = style.get_effects();
        if effects.contains(Effects::BOLD) {
            cell = cell.add_attribute(Attribute::Bold);
        }
        if effects.contains(Effects::DIMMED) {
            cell = cell.add_attribute(Attribute::Dim);
        }
        if effects == Effects::default() {
            cell = cell.add_attribute(Attribute::NormalIntensity);
        }
        cell
    }

    pub(super) fn table(&self, table: &mut Table) -> String {
        if self.theme.is_some() {
            // The output sink has already resolved auto/always/never. In particular,
            // --color always must override crossterm's independent NO_COLOR check.
            force_color_output(true);
            table.enforce_styling();
        }
        let text = table.to_string();
        // Every native styled cell starts with its background color. Reset border
        // intensity there, before the cell's own bold/dim attributes are emitted.
        // NormalIntensity cannot be combined with those attributes on Cell: the
        // backend emits it last, which would cancel the requested effects.
        let text = if self.theme.is_some() {
            text.replace("\x1b[49m", "\x1b[22m\x1b[49m")
                .replace("\x1b[48;", "\x1b[22m\x1b[48;")
        } else {
            text
        };
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

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;
    use hubuum_theme::{resolve_theme, ColorSpec, RoleStyle};

    use super::*;

    #[test]
    fn dimmed_cells_reset_border_intensity_before_applying_their_own_effects() {
        let mut theme = resolve_theme("hubuum-dark", None).unwrap();
        theme.roles.insert(
            ThemeRole::Border,
            RoleStyle::new(Some(ColorSpec::ansi(AnsiColor::Red)), None, true).with_dimmed(true),
        );
        let styling = OutputStyling::preview(&theme);
        let mut table = Table::new();
        table.add_row([
            styling.cell("null", styling.value_style("null")),
            styling.cell("ordinary", styling.value_style("ordinary")),
        ]);
        let text = styling.table(&mut table);
        // Clear inherited bold/dim before applying cell colors and the null's dim.
        assert!(
            text.contains("\x1b[22m\x1b[49m\x1b[38;5;8m\x1b[2m"),
            "{text:?}"
        );
        assert!(
            text.contains("\x1b[22m\x1b[49m\x1b[39m\x1b[22m"),
            "{text:?}"
        );
    }
}
