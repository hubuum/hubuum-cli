use std::fmt::Display;

use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS,
    presets::{ASCII_FULL, ASCII_MARKDOWN, UTF8_FULL, UTF8_HORIZONTAL_ONLY},
    ContentArrangement, Table,
};
use serde::Serialize;

use crate::{config::get_config, errors::AppError, models::TableStyle, output::append_line};

pub trait OutputFormatter: Sized + Serialize + Clone {
    fn format(&self) -> Result<Self, AppError>;

    fn format_noreturn(&self) -> Result<(), AppError> {
        self.format()?;
        Ok(())
    }

    fn format_json_noreturn(&self) -> Result<(), AppError> {
        append_json(self)?;
        Ok(())
    }
}

pub trait DetailRenderable {
    fn detail_rows(&self) -> Vec<(&'static str, String)>;
}

pub trait TableRenderable {
    fn headers() -> Vec<&'static str>;
    fn row(&self) -> Vec<String>;
}

impl<T> OutputFormatter for T
where
    T: DetailRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        for (key, value) in self.detail_rows() {
            append_key_value(key, value, padding)?;
        }
        Ok(self.clone())
    }
}

impl<T> OutputFormatter for Vec<T>
where
    T: TableRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        if self.is_empty() {
            append_line("No results.")?;
            return Ok(self.clone());
        }

        for line in render_table(self)?.lines() {
            append_line(line)?;
        }

        Ok(self.clone())
    }
}

pub fn append_json_message<V>(value: V) -> Result<(), AppError>
where
    V: Serialize,
{
    let message = serde_json::json!({ "message": value });
    append_line(serde_json::to_string_pretty(&message)?)?;
    Ok(())
}

pub fn append_json<T>(value: &T) -> Result<(), AppError>
where
    T: Serialize + ?Sized,
{
    append_line(serde_json::to_string_pretty(value)?)?;
    Ok(())
}

pub fn append_key_value<K, V>(key: K, value: V, padding: i8) -> Result<(), AppError>
where
    K: Display,
    V: Display,
{
    append_line(pad_key_value(key, value, padding))
}

fn pad_key_value<K, V>(key: K, value: V, padding: i8) -> String
where
    K: Display,
    V: Display,
{
    let padding = padding as usize;
    format!("{key:<padding$}: {value}")
}

fn render_table<T>(rows: &[T]) -> Result<String, AppError>
where
    T: TableRenderable,
{
    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(T::headers());

    apply_table_style(&mut table, &get_config().output.table_style);

    for row in rows {
        table.add_row(row.row());
    }

    Ok(table.to_string())
}

fn apply_table_style(table: &mut Table, style: &TableStyle) {
    match style {
        TableStyle::Ascii => {
            table.load_preset(ASCII_FULL);
        }
        TableStyle::Compact => {
            table.load_preset(UTF8_HORIZONTAL_ONLY);
        }
        TableStyle::Markdown => {
            table.load_preset(ASCII_MARKDOWN);
        }
        TableStyle::Rounded => {
            table.load_preset(UTF8_FULL);
            table.apply_modifier(UTF8_ROUND_CORNERS);
        }
    }
}
