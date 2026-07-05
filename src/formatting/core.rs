use std::fmt::Display;

use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS,
    presets::{ASCII_FULL, ASCII_MARKDOWN, NOTHING, UTF8_FULL, UTF8_HORIZONTAL_ONLY},
    ColumnConstraint, ContentArrangement, Table, Width,
};
use serde::Serialize;

use crate::{
    config::get_config,
    errors::AppError,
    models::{EmptyResult, TableStyle, TableWidth, TableWrap},
    output::append_line,
};

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
            if get_config().output.empty_result == EmptyResult::Message {
                append_line("No results.")?;
            }
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
    let config = get_config();
    let mut table = Table::new();
    table.set_header(T::headers());

    apply_table_style(&mut table, &config.output.table_style);
    apply_table_layout(
        &mut table,
        &config.output.table_width,
        &config.output.table_wrap,
        T::headers().len(),
    );

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
        TableStyle::Plain => {
            table.load_preset(NOTHING);
        }
        TableStyle::Rounded => {
            table.load_preset(UTF8_FULL);
            table.apply_modifier(UTF8_ROUND_CORNERS);
        }
    }
}

fn apply_table_layout(table: &mut Table, width: &TableWidth, wrap: &TableWrap, columns: usize) {
    let arrangement = match wrap {
        TableWrap::Never => ContentArrangement::Disabled,
        TableWrap::Auto | TableWrap::Fixed(_) => match width {
            TableWidth::Full => ContentArrangement::DynamicFullWidth,
            TableWidth::Auto | TableWidth::Fixed(_) => ContentArrangement::Dynamic,
        },
    };
    table.set_content_arrangement(arrangement);

    match width {
        TableWidth::Auto => {}
        TableWidth::Full => {
            if let Some(width) = terminal_width() {
                table.set_width(width);
            }
        }
        TableWidth::Fixed(width) => {
            table.set_width(*width);
        }
    }

    if let TableWrap::Fixed(width) = wrap {
        table.set_constraints(std::iter::repeat_n(
            ColumnConstraint::UpperBoundary(Width::Fixed(*width)),
            columns,
        ));
    }
}

fn terminal_width() -> Option<u16> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
}

#[cfg(test)]
mod tests {
    use super::{OutputFormatter, TableRenderable};
    use crate::{
        config::{init_config, AppConfig},
        models::{EmptyResult, TableStyle},
        output::{reset_output, take_output},
    };
    use serde::Serialize;
    use serial_test::serial;

    #[derive(Clone, Serialize)]
    struct Row {
        name: &'static str,
        value: &'static str,
    }

    impl TableRenderable for Row {
        fn headers() -> Vec<&'static str> {
            vec!["name", "value"]
        }

        fn row(&self) -> Vec<String> {
            vec![self.name.to_string(), self.value.to_string()]
        }
    }

    #[test]
    #[serial]
    fn empty_table_can_be_silent() {
        let mut config = AppConfig::default();
        config.output.empty_result = EmptyResult::Silent;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");

        Vec::<Row>::new().format_noreturn().expect("format");

        assert!(take_output().expect("snapshot").is_empty());
    }

    #[test]
    #[serial]
    fn plain_table_style_removes_borders() {
        let mut config = AppConfig::default();
        config.output.table_style = TableStyle::Plain;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");

        vec![Row {
            name: "alpha",
            value: "one",
        }]
        .format_noreturn()
        .expect("format");

        let rendered = take_output().expect("snapshot").render();
        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains('+'));
        assert!(!rendered.contains('│'));
    }
}
