use std::io::Write;

use anstream::AutoStream;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS,
    presets::{ASCII_FULL, ASCII_MARKDOWN, NOTHING, UTF8_FULL, UTF8_HORIZONTAL_ONLY},
    ColumnConstraint, ContentArrangement, Table, Width,
};
use hubuum_filter::{OutputEnvelope, OutputShape, PipeStage};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use std::fmt::Display;
use std::fmt::Write as FmtWrite;
use std::sync::Mutex;

use log::debug;

use crate::config::get_config;
use crate::errors::AppError;
use crate::models::{EmptyResult, TableBands, TableStyle, TableWidth, TableWrap};
use crate::terminal::terminal_width;
use crate::theme::{paint, ThemeRole};

static OUTPUT_BUFFER: Lazy<Mutex<OutputBuffer>> = Lazy::new(|| Mutex::new(OutputBuffer::new()));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub next_page_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderFormat {
    #[default]
    Text,
    Json,
    Jsonl,
    Csv,
    Tsv,
}

impl OutputSnapshot {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.warnings.is_empty() && self.errors.is_empty()
    }

    pub fn render(&self) -> String {
        let mut rendered = Vec::new();

        rendered.extend(
            self.warnings
                .iter()
                .map(|warning| paint(ThemeRole::Warning, format!("Warning: {warning}"))),
        );
        rendered.extend(
            self.errors
                .iter()
                .map(|error| paint(ThemeRole::Error, format!("Error: {error}"))),
        );
        rendered.extend(self.lines.iter().cloned());

        if rendered.is_empty() {
            String::new()
        } else {
            format!("{}\n", rendered.join("\n"))
        }
    }
}

pub fn print_rendered(text: &str) -> Result<(), AppError> {
    let stdout = std::io::stdout();
    let mut stream = AutoStream::new(stdout, crate::theme::color_choice());
    stream.write_all(text.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct OutputBuffer {
    lines: Vec<String>,
    semantic: Vec<OutputEnvelope>,
    pipeline: Vec<PipeStage>,
    render_format: RenderFormat,
    warnings: Vec<String>,
    errors: Vec<String>,
    next_page_command: Option<String>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            render_format: config_render_format(),
            ..Self::default()
        }
    }

    fn add_warning(&mut self, message: String) {
        self.warnings.push(message);
    }

    fn add_error(&mut self, message: String) {
        self.errors.push(message);
    }

    fn append_line(&mut self, line: String) {
        self.lines.push(line);
    }

    fn set_semantic(&mut self, envelope: OutputEnvelope) {
        self.semantic.push(envelope);
    }

    fn set_pipeline(&mut self, stages: Vec<PipeStage>) {
        debug!("Setting output pipeline: {stages:?}");
        self.pipeline = stages;
    }

    fn set_render_format(&mut self, format: RenderFormat) {
        self.render_format = format;
    }

    fn set_next_page_command(&mut self, command: String) {
        self.next_page_command = Some(command);
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.semantic.clear();
        self.warnings.clear();
        self.errors.clear();
        self.pipeline.clear();
        self.render_format = config_render_format();
        self.next_page_command = None;
    }

    fn snapshot(&self) -> Result<OutputSnapshot, AppError> {
        let lines = if !self.semantic.is_empty() {
            let mut rendered = self.lines.clone();
            for envelope in &self.semantic {
                let envelope = hubuum_filter::apply_pipeline(envelope.clone(), &self.pipeline)?;
                rendered.extend(render_semantic(&envelope, self.render_format)?);
            }
            rendered
        } else {
            PipeStage::apply_all(&self.pipeline, self.lines.clone())?
        };

        Ok(OutputSnapshot {
            lines,
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
            next_page_command: self.next_page_command.clone(),
        })
    }

    fn take_snapshot(&mut self) -> Result<OutputSnapshot, AppError> {
        let snapshot = self.snapshot();
        self.reset();
        snapshot
    }
}

pub fn add_warning<T: Display>(message: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .add_warning(message.to_string());
    Ok(())
}

pub fn add_error<T: Display>(message: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .add_error(message.to_string());
    Ok(())
}

pub fn append_line<T: Display>(line: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .append_line(line.to_string());
    Ok(())
}

pub fn set_semantic_output(envelope: OutputEnvelope) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_semantic(envelope);
    Ok(())
}

#[allow(dead_code)]
pub fn append_lines<T: Display>(lines: &[T]) -> Result<(), AppError> {
    let mut buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;
    for line in lines {
        buffer.append_line(line.to_string());
    }
    Ok(())
}

#[allow(dead_code)]
pub fn append_debug<T: std::fmt::Debug>(value: T) -> Result<(), AppError> {
    let mut debug_output = String::new();
    write!(&mut debug_output, "{value:#?}").map_err(|_| AppError::FormatError)?;

    let mut output_buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;

    for line in debug_output.lines() {
        output_buffer.append_line(line.to_string());
    }

    Ok(())
}

#[allow(dead_code)]
pub fn append_json<T: Serialize>(value: T) -> Result<(), AppError> {
    set_semantic_output(OutputEnvelope::detail(
        serde_json::to_value(value)?,
        Vec::new(),
    ))
}

pub fn append_key_value<K: Display, V: Display>(
    key: K,
    value: V,
    padding: usize,
) -> Result<(), AppError> {
    let line = format!("{key:<padding$} : {value}");
    append_line(line)
}

pub fn reset_output() -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .reset();
    Ok(())
}

pub fn take_output() -> Result<OutputSnapshot, AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .take_snapshot()
}

pub fn set_pipeline(stages: Vec<PipeStage>) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_pipeline(stages);
    Ok(())
}

pub fn set_render_format(format: RenderFormat) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_render_format(format);
    Ok(())
}

pub fn set_next_page_command(command: String) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_next_page_command(command);
    Ok(())
}

fn render_semantic(
    envelope: &OutputEnvelope,
    format: RenderFormat,
) -> Result<Vec<String>, AppError> {
    match format {
        RenderFormat::Text => render_semantic_text(envelope),
        RenderFormat::Json => Ok(serde_json::to_string_pretty(&envelope.value)?
            .lines()
            .map(str::to_string)
            .collect()),
        RenderFormat::Jsonl => Ok(render_jsonl(&envelope.value)?),
        RenderFormat::Csv => render_delimited(envelope, ','),
        RenderFormat::Tsv => render_delimited(envelope, '\t'),
    }
}

fn config_render_format() -> RenderFormat {
    match get_config().output.format {
        crate::models::OutputFormat::Json => RenderFormat::Json,
        crate::models::OutputFormat::Text => RenderFormat::Text,
    }
}

fn render_semantic_text(envelope: &OutputEnvelope) -> Result<Vec<String>, AppError> {
    match envelope.shape {
        OutputShape::Empty => Ok(Vec::new()),
        OutputShape::Lines => Ok(value_array(&envelope.value)
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()),
        OutputShape::Rows => render_rows_text(envelope),
        OutputShape::Detail => render_detail_text(envelope),
        OutputShape::Message => Ok(vec![semantic_scalar(&envelope.value)]),
        OutputShape::Values => Ok(value_array(&envelope.value)
            .iter()
            .map(semantic_scalar)
            .collect()),
    }
}

fn render_rows_text(envelope: &OutputEnvelope) -> Result<Vec<String>, AppError> {
    let rows = value_array(&envelope.value);
    if rows.is_empty() {
        return if get_config().output.empty_result == EmptyResult::Message {
            Ok(vec!["No results.".to_string()])
        } else {
            Ok(Vec::new())
        };
    }

    let columns = display_columns(envelope, &rows);
    if get_config().output.table_style == TableStyle::Dense {
        return Ok(render_dense_rows(&rows, &columns));
    }

    let headers = column_headers(&columns);
    let mut table = Table::new();
    table.set_header(headers);
    apply_table_style(&mut table, &get_config().output.table_style);
    apply_table_layout(
        &mut table,
        &get_config().output.table_width,
        &get_config().output.table_wrap,
        columns.len(),
    );

    for row in rows {
        table.add_row(
            columns
                .iter()
                .map(|column| cell_text(row.get(column)))
                .collect::<Vec<_>>(),
        );
    }

    Ok(table.to_string().lines().map(str::to_string).collect())
}

fn render_detail_text(envelope: &OutputEnvelope) -> Result<Vec<String>, AppError> {
    let columns = if envelope.columns.is_empty() {
        envelope
            .value
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default()
    } else {
        envelope.columns.clone()
    };
    let padding = get_config().output.padding as usize;
    Ok(columns
        .iter()
        .map(|column| {
            format!(
                "{column:<padding$}: {}",
                cell_text(envelope.value.get(column))
            )
        })
        .collect())
}

fn render_jsonl(value: &Value) -> Result<Vec<String>, AppError> {
    if let Value::Array(items) = value {
        items
            .iter()
            .map(|item| serde_json::to_string(item).map_err(AppError::from))
            .collect()
    } else {
        Ok(vec![serde_json::to_string(value)?])
    }
}

fn render_delimited(envelope: &OutputEnvelope, delimiter: char) -> Result<Vec<String>, AppError> {
    let rows = match envelope.shape {
        OutputShape::Rows => value_array(&envelope.value),
        OutputShape::Detail | OutputShape::Message => vec![envelope.value.clone()],
        OutputShape::Values => value_array(&envelope.value)
            .into_iter()
            .map(|value| serde_json::json!({ "value": value }))
            .collect(),
        OutputShape::Empty | OutputShape::Lines => Vec::new(),
    };

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let columns = display_columns(envelope, &rows);
    let mut lines = vec![join_delimited(
        columns.iter().map(String::as_str),
        delimiter,
    )];
    lines.extend(rows.iter().map(|row| {
        join_delimited(
            columns
                .iter()
                .map(|column| cell_text(row.get(column)))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str),
            delimiter,
        )
    }));
    Ok(lines)
}

fn join_delimited<'a>(values: impl IntoIterator<Item = &'a str>, delimiter: char) -> String {
    values
        .into_iter()
        .map(|value| escape_delimited(value, delimiter))
        .collect::<Vec<_>>()
        .join(&delimiter.to_string())
}

fn escape_delimited(value: &str, delimiter: char) -> String {
    if delimiter == '\t' {
        return value.replace(['\t', '\n', '\r'], " ");
    }

    if value.contains([delimiter, '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn display_columns(envelope: &OutputEnvelope, rows: &[Value]) -> Vec<String> {
    if !envelope.columns.is_empty() {
        return envelope.columns.clone();
    }
    rows.iter()
        .find_map(Value::as_object)
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_else(|| vec!["value".to_string()])
}

fn render_dense_rows(rows: &[Value], columns: &[String]) -> Vec<String> {
    let headers = column_headers(columns);
    let widths = dense_widths(rows, columns, &headers);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(render_dense_line(
        headers.iter().map(String::as_str),
        &widths,
    ));
    for (index, row) in rows.iter().enumerate() {
        let line = render_dense_line(
            columns
                .iter()
                .map(|column| cell_text(row.get(column)))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str),
            &widths,
        );
        lines.push(apply_row_band(index, line));
    }
    lines
}

fn dense_widths(rows: &[Value], columns: &[String], headers: &[String]) -> Vec<usize> {
    columns
        .iter()
        .zip(headers.iter())
        .map(|column| {
            let (column, header) = column;
            rows.iter()
                .map(|row| cell_text(row.get(column)).len())
                .chain(std::iter::once(header.len()))
                .max()
                .unwrap_or(header.len())
        })
        .collect()
}

fn column_headers(columns: &[String]) -> Vec<String> {
    columns.iter().map(|column| column_header(column)).collect()
}

fn column_header(column: &str) -> String {
    column.strip_prefix("data.").unwrap_or(column).to_string()
}

fn render_dense_line<'a>(values: impl IntoIterator<Item = &'a str>, widths: &[usize]) -> String {
    values
        .into_iter()
        .zip(widths.iter())
        .map(|(value, width)| format!("{value:<width$}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn apply_row_band(index: usize, line: String) -> String {
    match get_config().output.table_bands {
        TableBands::Never => line,
        TableBands::Auto | TableBands::Always => {
            if index % 2 == 1 {
                paint(ThemeRole::TableBand, line)
            } else {
                line
            }
        }
    }
}

fn value_array(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn cell_text(value: Option<&Value>) -> String {
    value.map(semantic_scalar).unwrap_or_default()
}

fn semantic_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
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
        TableStyle::Plain | TableStyle::Dense => {
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
            if let Some(width) = terminal_width().and_then(|width| u16::try_from(width).ok()) {
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

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::{append_line, reset_output, set_pipeline, set_semantic_output, take_output};
    use crate::config::{init_config, AppConfig};
    use crate::models::{OutputColor, TableStyle};
    use hubuum_filter::{OutputEnvelope, PipeStage};
    #[test]
    #[serial]
    fn take_output_applies_filter_and_resets_buffer() {
        reset_output().expect("buffer should reset");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");
        set_pipeline(vec![PipeStage::Grep("^b".to_string())]).expect("pipeline should set");

        let snapshot = take_output().expect("snapshot should be available");
        assert_eq!(snapshot.lines, vec!["beta".to_string()]);

        let empty = take_output().expect("buffer should be empty after take");
        assert!(empty.is_empty());
    }

    #[test]
    #[serial]
    fn render_honors_never_color() {
        let mut config = AppConfig::default();
        config.output.color = OutputColor::Never;
        init_config(config).expect("config should initialize");

        let snapshot = super::OutputSnapshot {
            warnings: vec!["careful".to_string()],
            errors: vec!["failed".to_string()],
            ..Default::default()
        };

        assert_eq!(snapshot.render(), "Warning: careful\nError: failed\n");
    }

    #[test]
    #[serial]
    fn structured_pipeline_ignores_auxiliary_lines_when_semantic_output_exists() {
        reset_output().expect("buffer should reset");
        append_line("Returned 1 item(s)").expect("line should append");
        set_semantic_output(OutputEnvelope::rows(
            vec![serde_json::json!({"Name": "alpha", "hidden": "secret"})],
            vec!["Name".to_string(), "hidden".to_string()],
        ))
        .expect("semantic output should be set");
        set_pipeline(vec![PipeStage::Columns(vec!["Name".to_string()])])
            .expect("pipeline should set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered.contains("Returned 1 item(s)"));
        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    #[serial]
    fn text_tables_shorten_data_prefixed_headers() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![serde_json::json!({"Name": "alpha", "data.contact": "Entry"})],
            vec!["Name".to_string(), "data.contact".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered.contains("contact"));
        assert!(!rendered.contains("data.contact"));
        assert!(rendered.contains("Entry"));
    }

    #[test]
    #[serial]
    fn dense_tables_shorten_data_prefixed_headers() {
        let mut config = AppConfig::default();
        config.output.table_style = TableStyle::Dense;
        init_config(config).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![serde_json::json!({"Name": "alpha", "data.contact": "Entry"})],
            vec!["Name".to_string(), "data.contact".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered
            .lines()
            .next()
            .is_some_and(|line| line.contains("contact")));
        assert!(!rendered.contains("data.contact"));
        assert!(rendered.contains("Entry"));
    }
}
