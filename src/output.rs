use std::fmt::{Debug, Display, Write as FmtWrite};
use std::io::{stdout, Write};
use std::iter::{once, repeat_n};

use anstream::AutoStream;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS,
    presets::{ASCII_FULL, ASCII_MARKDOWN, NOTHING, UTF8_FULL, UTF8_HORIZONTAL_ONLY},
    ColumnConstraint, ContentArrangement, Table, Width,
};
use hubuum_filter::{apply_pipeline, group_summary_rows, OutputEnvelope, OutputShape, PipeStage};
use hubuum_theme::{paint as paint_theme, Theme as HubuumTheme};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::{json, to_string, to_string_pretty, to_value, Value};
use std::sync::Mutex;

use log::debug;

use crate::config::get_config;
use crate::errors::AppError;
use crate::models::{EmptyResult, OutputFormat, TableBands, TableStyle, TableWidth, TableWrap};
use crate::terminal::terminal_width;
use crate::theme::{color_choice, paint, ThemeRole};

static OUTPUT_BUFFER: Lazy<Mutex<OutputBuffer>> = Lazy::new(|| Mutex::new(OutputBuffer::new()));

#[derive(Debug)]
enum OutputEvent {
    Line(String),
    Semantic(OutputEnvelope),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputSnapshot {
    pub lines: Vec<String>,
    pub semantic: Vec<OutputEnvelope>,
    pub render_format: RenderFormat,
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
    let stdout = stdout();
    let mut stream = AutoStream::new(stdout, color_choice());
    stream.write_all(text.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct OutputBuffer {
    events: Vec<OutputEvent>,
    pipeline: Vec<PipeStage>,
    pipeline_suffix: Option<String>,
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
        self.events.push(OutputEvent::Line(line));
    }

    fn set_semantic(&mut self, envelope: OutputEnvelope) {
        self.events.push(OutputEvent::Semantic(envelope));
    }

    fn set_pipeline(&mut self, stages: Vec<PipeStage>) {
        debug!("Setting output pipeline: {stages:?}");
        self.pipeline = stages;
    }

    fn set_pipeline_suffix(&mut self, suffix: Option<String>) {
        self.pipeline_suffix = suffix;
    }

    fn append_pipeline_suffix(&self, command: String) -> String {
        match &self.pipeline_suffix {
            Some(suffix) => format!("{command} {suffix}"),
            None => command,
        }
    }

    fn has_pipeline(&self) -> bool {
        !self.pipeline.is_empty()
    }

    fn set_render_format(&mut self, format: RenderFormat) {
        self.render_format = format;
    }

    fn set_next_page_command(&mut self, command: String) {
        self.next_page_command = Some(command);
    }

    fn pipeline_suppresses_pagination(&self) -> bool {
        self.pipeline.iter().any(|stage| {
            matches!(
                stage,
                PipeStage::Head { .. }
                    | PipeStage::Tail(_)
                    | PipeStage::Count
                    | PipeStage::Group(_)
                    | PipeStage::Aggregate(_)
                    | PipeStage::CollapseGroups
                    | PipeStage::Jq(_)
                    | PipeStage::Value(_)
            )
        })
    }

    fn reset(&mut self) {
        self.events.clear();
        self.warnings.clear();
        self.errors.clear();
        self.pipeline.clear();
        self.pipeline_suffix = None;
        self.render_format = config_render_format();
        self.next_page_command = None;
    }

    fn snapshot(&self) -> Result<OutputSnapshot, AppError> {
        let mut semantic = Vec::new();
        let has_semantic = self
            .events
            .iter()
            .any(|event| matches!(event, OutputEvent::Semantic(_)));
        let lines = if has_semantic {
            let mut rendered = Vec::new();
            for event in &self.events {
                match event {
                    OutputEvent::Line(line) => rendered.push(line.clone()),
                    OutputEvent::Semantic(envelope) => {
                        let envelope = apply_pipeline(envelope.clone(), &self.pipeline)?;
                        rendered.extend(render_semantic(&envelope, self.render_format)?);
                        semantic.push(envelope);
                    }
                }
            }
            rendered
        } else {
            let lines = self
                .events
                .iter()
                .filter_map(|event| match event {
                    OutputEvent::Line(line) => Some(line.clone()),
                    OutputEvent::Semantic(_) => None,
                })
                .collect();
            PipeStage::apply_all(&self.pipeline, lines)?
        };

        Ok(OutputSnapshot {
            lines,
            semantic,
            render_format: self.render_format,
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
pub fn append_debug<T: Debug>(value: T) -> Result<(), AppError> {
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
    set_semantic_output(OutputEnvelope::detail(to_value(value)?, Vec::new()))
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

pub fn set_pipeline_suffix(suffix: Option<String>) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_pipeline_suffix(suffix);
    Ok(())
}

pub fn append_pipeline_suffix(command: String) -> Result<String, AppError> {
    Ok(OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .append_pipeline_suffix(command))
}

pub fn has_pipeline() -> Result<bool, AppError> {
    Ok(OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .has_pipeline())
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

pub fn pipeline_suppresses_pagination() -> Result<bool, AppError> {
    Ok(OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .pipeline_suppresses_pagination())
}

pub(crate) fn render_semantic(
    envelope: &OutputEnvelope,
    format: RenderFormat,
) -> Result<Vec<String>, AppError> {
    match format {
        RenderFormat::Text => render_semantic_text(envelope),
        RenderFormat::Json => Ok(to_string_pretty(&envelope.value)?
            .lines()
            .map(str::to_string)
            .collect()),
        RenderFormat::Jsonl => Ok(render_jsonl(&envelope.value)?),
        RenderFormat::Csv => render_delimited(envelope, ','),
        RenderFormat::Tsv => render_delimited(envelope, '\t'),
    }
}

pub(crate) fn render_semantic_item(
    value: &Value,
    source_shape: OutputShape,
    columns: &[String],
    format: RenderFormat,
) -> Result<String, AppError> {
    let lines = match format {
        RenderFormat::Text => match source_shape {
            OutputShape::Rows | OutputShape::Detail | OutputShape::Message => {
                render_detail_text(&OutputEnvelope::detail(value.clone(), columns.to_vec()))?
            }
            OutputShape::Values | OutputShape::Lines => vec![semantic_scalar(value)],
            OutputShape::Groups => {
                render_rows_text(&OutputEnvelope::rows(group_summary_rows(value), Vec::new()))?
            }
            OutputShape::Empty => Vec::new(),
        },
        RenderFormat::Json => to_string_pretty(value)?
            .lines()
            .map(str::to_string)
            .collect(),
        RenderFormat::Jsonl => vec![to_string(value)?],
        RenderFormat::Csv => render_item_delimited(value, source_shape, columns, ',')?,
        RenderFormat::Tsv => render_item_delimited(value, source_shape, columns, '\t')?,
    };

    Ok(if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    })
}

fn config_render_format() -> RenderFormat {
    match get_config().output.format {
        OutputFormat::Json => RenderFormat::Json,
        OutputFormat::Text => RenderFormat::Text,
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
        OutputShape::Groups => render_rows_text(&OutputEnvelope::rows(
            group_summary_rows(&envelope.value),
            envelope.columns.clone(),
        )),
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
            .map(|item| to_string(item).map_err(AppError::from))
            .collect()
    } else {
        Ok(vec![to_string(value)?])
    }
}

fn render_delimited(envelope: &OutputEnvelope, delimiter: char) -> Result<Vec<String>, AppError> {
    let rows = match envelope.shape {
        OutputShape::Rows => value_array(&envelope.value),
        OutputShape::Detail | OutputShape::Message => vec![envelope.value.clone()],
        OutputShape::Values => value_array(&envelope.value)
            .into_iter()
            .map(|value| json!({ "value": value }))
            .collect(),
        OutputShape::Groups => group_summary_rows(&envelope.value),
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

fn render_item_delimited(
    value: &Value,
    source_shape: OutputShape,
    columns: &[String],
    delimiter: char,
) -> Result<Vec<String>, AppError> {
    let envelope = match source_shape {
        OutputShape::Rows | OutputShape::Detail | OutputShape::Message => {
            OutputEnvelope::detail(value.clone(), columns.to_vec())
        }
        OutputShape::Values | OutputShape::Lines => {
            OutputEnvelope::rows(vec![json!({ "value": value })], vec!["value".to_string()])
        }
        OutputShape::Groups => OutputEnvelope::rows(group_summary_rows(value), columns.to_vec()),
        OutputShape::Empty => OutputEnvelope::empty(),
    };
    render_delimited(&envelope, delimiter)
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
    render_dense_rows_with_band(rows, columns, apply_row_band)
}

fn render_dense_rows_with_band(
    rows: &[Value],
    columns: &[String],
    mut band_row: impl FnMut(usize, String) -> String,
) -> Vec<String> {
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
        lines.push(band_row(index, line));
    }
    lines
}

pub(crate) fn render_dense_theme_preview(theme: &HubuumTheme) -> Vec<String> {
    let rows = vec![
        json!({"Name": "edge-gateway-01", "os_version": "Debian 13", "status": "Ready"}),
        json!({"Name": "build-runner-04", "os_version": "Ubuntu 26.04", "status": "Busy"}),
        json!({"Name": "storage-node-02", "os_version": "Rocky 10", "status": "Ready"}),
        json!({"Name": "lab-console-07", "os_version": "Fedora 44", "status": "Offline"}),
    ];
    let columns = vec![
        "Name".to_string(),
        "os_version".to_string(),
        "status".to_string(),
    ];

    render_dense_rows_with_band(&rows, &columns, |index, line| {
        if index.is_multiple_of(2) {
            paint_theme(theme, ThemeRole::TableBand, line)
        } else {
            line
        }
    })
}

fn dense_widths(rows: &[Value], columns: &[String], headers: &[String]) -> Vec<usize> {
    columns
        .iter()
        .zip(headers.iter())
        .map(|column| {
            let (column, header) = column;
            rows.iter()
                .map(|row| cell_text(row.get(column)).len())
                .chain(once(header.len()))
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
            if index.is_multiple_of(2) {
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
    match value {
        Some(Value::Null) | None => String::new(),
        Some(value) => semantic_scalar(value),
    }
}

fn semantic_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => to_string(value).unwrap_or_default(),
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
        table.set_constraints(repeat_n(
            ColumnConstraint::UpperBoundary(Width::Fixed(*width)),
            columns,
        ));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serial_test::serial;

    use super::{
        append_line, render_dense_theme_preview, reset_output, set_pipeline, set_render_format,
        set_semantic_output, take_output, OutputSnapshot, RenderFormat,
    };
    use crate::config::{init_config, AppConfig};
    use crate::models::{OutputColor, TableBands, TableStyle};
    use hubuum_filter::{OutputEnvelope, PipeStage, ProjectTerm};
    use hubuum_theme::resolve_theme;
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

        let snapshot = OutputSnapshot {
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
            vec![json!({"Name": "alpha", "hidden": "secret"})],
            vec!["Name".to_string(), "hidden".to_string()],
        ))
        .expect("semantic output should be set");
        set_pipeline(vec![PipeStage::Columns(vec![ProjectTerm::keep("Name")])])
            .expect("pipeline should set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered.contains("Returned 1 item(s)"));
        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    #[serial]
    fn mixed_output_preserves_insertion_order() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({"Name": "alpha"})],
            vec!["Name".to_string()],
        ))
        .expect("semantic output should be set");
        append_line("Returned 1 item(s)").expect("footer should append");

        let rendered = take_output().expect("snapshot").render();
        let row = rendered.find("alpha").expect("row should render");
        let footer = rendered
            .find("Returned 1 item(s)")
            .expect("footer should render");

        assert!(row < footer);
    }

    #[test]
    #[serial]
    fn json_rendering_applies_projection_to_semantic_rows_before_rendering() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_render_format(RenderFormat::Json).expect("render format should set");
        set_pipeline(vec![PipeStage::Columns(vec![
            ProjectTerm::keep("Name"),
            ProjectTerm::keep("data.network.interfaces[*].ipv4"),
        ])])
        .expect("pipeline should set");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({
                "Name": "host-1",
                "data": {
                    "network": {
                        "interfaces": [
                            {"ipv4": "127.0.0.1"},
                            {"ipv4": "127.0.0.2"}
                        ]
                    }
                },
                "hidden": "secret"
            })],
            vec!["Name".to_string(), "hidden".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered.contains("\"Name\": \"host-1\""));
        assert!(rendered.contains("\"data.network.interfaces[*].ipv4\""));
        assert!(rendered.contains("\"127.0.0.1\""));
        assert!(rendered.contains("\"127.0.0.2\""));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    #[serial]
    fn text_tables_shorten_data_prefixed_headers() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({"Name": "alpha", "data.contact": "Entry"})],
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
            vec![json!({"Name": "alpha", "data.contact": "Entry"})],
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

    #[test]
    #[serial]
    fn dense_table_bands_use_subtle_dark_theme_background() {
        let mut config = AppConfig::default();
        config.output.color = OutputColor::Always;
        config.output.table_style = TableStyle::Dense;
        config.output.table_bands = TableBands::Always;
        init_config(config).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({"Name": "alpha"}), json!({"Name": "beta"})],
            vec!["Name".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();
        let lines = rendered.lines().collect::<Vec<_>>();

        assert!(!lines[0].contains("\x1b[48;5;236m"));
        assert!(lines[1].contains("\x1b[48;5;236m"));
        assert!(lines[1].contains("alpha"));
        assert!(!lines[2].contains("\x1b[48;5;236m"));
        assert!(lines[2].contains("beta"));
    }

    #[test]
    #[serial]
    fn dense_table_bands_respect_never_color() {
        let mut config = AppConfig::default();
        config.output.color = OutputColor::Never;
        config.output.table_style = TableStyle::Dense;
        config.output.table_bands = TableBands::Always;
        init_config(config).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({"Name": "alpha"}), json!({"Name": "beta"})],
            vec!["Name".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert!(!rendered.contains("\x1b[48;5;236m"));
        assert!(rendered.contains("beta"));
    }

    #[test]
    fn dense_theme_preview_bands_alternating_rows() {
        let theme = resolve_theme("rose-pink", None).expect("rose-pink theme");
        let lines = render_dense_theme_preview(&theme);

        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("Name"));
        assert!(lines[1].contains('\u{1b}'));
        assert!(!lines[2].contains('\u{1b}'));
        assert!(lines[3].contains('\u{1b}'));
        assert!(!lines[4].contains('\u{1b}'));
        assert!(lines[1].contains("edge-gateway-01"));
        assert!(lines[3].contains("storage-node-02"));
    }

    #[test]
    #[serial]
    fn text_tables_render_null_cells_as_blank() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({"Name": "alpha", "os_version": null})],
            vec!["Name".to_string(), "os_version".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains("null"));
    }
}
