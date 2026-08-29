use std::collections::{BTreeSet, HashMap};
use std::fmt::{Debug, Display, Write as FmtWrite};
use std::io::{stdout, Write};
use std::iter::{once, repeat_n};

use anstream::AutoStream;
use comfy_table::{
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
use crate::models::{
    EmptyResult, OutputFormat, TableBands, TableHeaders, TableStyle, TableWidth, TableWrap,
};
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
    table_headers: TableHeaders,
    warnings: Vec<String>,
    errors: Vec<String>,
    next_page_command: Option<String>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            render_format: config_render_format(),
            table_headers: get_config().output.table_headers,
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

    fn set_table_headers(&mut self, table_headers: TableHeaders) {
        self.table_headers = table_headers;
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
        self.table_headers = get_config().output.table_headers;
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
                        rendered.extend(render_semantic_with_table_headers(
                            &envelope,
                            self.render_format,
                            self.table_headers,
                        )?);
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

pub fn set_table_headers(table_headers: TableHeaders) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_table_headers(table_headers);
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

fn render_semantic_with_table_headers(
    envelope: &OutputEnvelope,
    format: RenderFormat,
    table_headers: TableHeaders,
) -> Result<Vec<String>, AppError> {
    match format {
        RenderFormat::Text => render_semantic_text(envelope, table_headers),
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
            OutputShape::Groups => render_rows_text(
                &OutputEnvelope::rows(group_summary_rows(value), Vec::new()),
                get_config().output.table_headers,
            )?,
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

fn render_semantic_text(
    envelope: &OutputEnvelope,
    table_headers: TableHeaders,
) -> Result<Vec<String>, AppError> {
    match envelope.shape {
        OutputShape::Empty => Ok(Vec::new()),
        OutputShape::Lines => Ok(value_array(&envelope.value)
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()),
        OutputShape::Rows => render_rows_text(envelope, table_headers),
        OutputShape::Detail => render_detail_text(envelope),
        OutputShape::Message => Ok(vec![semantic_scalar(&envelope.value)]),
        OutputShape::Values => Ok(value_array(&envelope.value)
            .iter()
            .map(semantic_scalar)
            .collect()),
        OutputShape::Groups => render_rows_text(
            &OutputEnvelope::rows(
                group_summary_rows(&envelope.value),
                envelope.columns.clone(),
            ),
            table_headers,
        ),
    }
}

fn render_rows_text(
    envelope: &OutputEnvelope,
    table_headers: TableHeaders,
) -> Result<Vec<String>, AppError> {
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
        return Ok(render_dense_rows(&rows, &columns, table_headers));
    }

    let headers = column_headers(&columns, &rows, table_headers);
    let mut table = Table::new();
    if !headers.is_empty() {
        table.set_header(headers);
    }
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
    let configured_padding = usize::try_from(get_config().output.padding).unwrap_or_default();
    let padding = columns
        .iter()
        .map(String::len)
        .max()
        .unwrap_or_default()
        .max(configured_padding);
    Ok(columns
        .iter()
        .map(|column| render_detail_field(column, &cell_text(envelope.value.get(column)), padding))
        .collect())
}

fn render_detail_field(column: &str, value: &str, padding: usize) -> String {
    let mut lines = value.split('\n');
    let first = lines.next().unwrap_or_default();
    let mut rendered = format!("{column:<padding$}: {first}");
    let continuation_indent = " ".repeat(padding + 2);
    for line in lines {
        rendered.push('\n');
        rendered.push_str(&continuation_indent);
        rendered.push_str(line);
    }
    rendered
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

fn render_dense_rows(
    rows: &[Value],
    columns: &[String],
    table_headers: TableHeaders,
) -> Vec<String> {
    let config = get_config();
    render_dense_rows_with_band(
        rows,
        columns,
        table_headers,
        DenseLayout::new(
            &config.output.table_width,
            &config.output.table_wrap,
            terminal_width(),
        ),
        apply_row_band,
    )
}

fn render_dense_rows_with_band(
    rows: &[Value],
    columns: &[String],
    table_headers: TableHeaders,
    layout: DenseLayout<'_>,
    mut band_row: impl FnMut(usize, String) -> String,
) -> Vec<String> {
    let headers = column_headers(columns, rows, table_headers);
    let widths = dense_widths(rows, columns, &headers, layout);
    let mut lines = render_dense_headers(&headers, &widths);
    lines.reserve(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let cells = columns
            .iter()
            .map(|column| cell_text(row.get(column)))
            .collect::<Vec<_>>();
        lines.extend(
            render_dense_cells(&cells, &widths)
                .into_iter()
                .map(|line| band_row(index, line)),
        );
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

    let config = get_config();
    render_dense_rows_with_band(
        &rows,
        &columns,
        config.output.table_headers,
        DenseLayout::new(
            &config.output.table_width,
            &config.output.table_wrap,
            terminal_width(),
        ),
        |index, line| {
            if index.is_multiple_of(2) {
                paint_theme(theme, ThemeRole::TableBand, line)
            } else {
                line
            }
        },
    )
}

#[derive(Clone, Copy)]
struct DenseLayout<'a> {
    width: &'a TableWidth,
    wrap: &'a TableWrap,
    terminal_width: Option<usize>,
}

impl<'a> DenseLayout<'a> {
    fn new(width: &'a TableWidth, wrap: &'a TableWrap, terminal_width: Option<usize>) -> Self {
        Self {
            width,
            wrap,
            terminal_width,
        }
    }

    fn constrain(self, widths: &mut [usize]) {
        match self.wrap {
            TableWrap::Never => return,
            TableWrap::Auto => {}
            TableWrap::Fixed(width) => {
                let cell_width = usize::from(*width).max(1);
                for width in widths.iter_mut() {
                    *width = (*width).min(cell_width);
                }
            }
        }

        let table_width = match self.width {
            TableWidth::Auto | TableWidth::Full => self.terminal_width,
            TableWidth::Fixed(width) => Some(usize::from(*width)),
        };
        if let Some(table_width) = table_width {
            constrain_dense_widths(widths, table_width);
        }
    }
}

fn dense_widths(
    rows: &[Value],
    columns: &[String],
    headers: &[String],
    layout: DenseLayout<'_>,
) -> Vec<usize> {
    let mut widths = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            rows.iter()
                .map(|row| dense_text_width(&cell_text(row.get(column))))
                .chain(once(
                    headers
                        .get(index)
                        .map(|header| dense_text_width(header))
                        .unwrap_or_default(),
                ))
                .max()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    layout.constrain(&mut widths);
    widths
}

fn dense_text_width(value: &str) -> usize {
    value
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default()
}

fn constrain_dense_widths(widths: &mut [usize], table_width: usize) {
    if widths.is_empty() {
        return;
    }
    let separator_width = 3_usize.saturating_mul(widths.len().saturating_sub(1));
    let available = table_width
        .saturating_sub(separator_width)
        .max(widths.len());
    let mut excess = widths.iter().sum::<usize>().saturating_sub(available);

    while excess > 0 {
        let widest = widths.iter().copied().max().unwrap_or(1);
        if widest <= 1 {
            break;
        }
        let next_widest = widths
            .iter()
            .copied()
            .filter(|width| *width < widest)
            .max()
            .unwrap_or(1);
        let widest_columns = widths
            .iter()
            .enumerate()
            .filter_map(|(index, width)| (*width == widest).then_some(index))
            .collect::<Vec<_>>();
        let reducible = (widest - next_widest).saturating_mul(widest_columns.len());
        let reduction = excess.min(reducible);
        if reduction == 0 {
            break;
        }
        let reduction_per_column = reduction / widest_columns.len();
        let remainder = reduction % widest_columns.len();
        for (position, index) in widest_columns.into_iter().enumerate() {
            widths[index] -= reduction_per_column + usize::from(position < remainder);
        }
        excess -= reduction;
    }
}

fn render_dense_headers(headers: &[String], widths: &[usize]) -> Vec<String> {
    render_dense_cells(headers, widths)
}

fn column_headers(columns: &[String], rows: &[Value], table_headers: TableHeaders) -> Vec<String> {
    match table_headers {
        TableHeaders::Full => {
            return columns.iter().map(|column| column_header(column)).collect();
        }
        TableHeaders::None => return Vec::new(),
        TableHeaders::Grouped => {}
    }

    let aliases = configured_header_aliases(columns, rows);
    let markdown = get_config().output.table_style == TableStyle::Markdown;
    columns
        .iter()
        .map(|column| {
            aliases.get(column).cloned().unwrap_or_else(|| {
                let header = column_header(column);
                if markdown {
                    header
                } else {
                    header.replace('.', "\n")
                }
            })
        })
        .collect()
}

fn configured_header_aliases(columns: &[String], rows: &[Value]) -> HashMap<String, String> {
    let Some(class_name) = common_object_class(rows) else {
        return HashMap::new();
    };
    let config = get_config();
    let Some(configured) = config.output.object_list_class_aliases.get(class_name) else {
        return HashMap::new();
    };

    let candidates = columns
        .iter()
        .map(|column| {
            let matches = configured
                .iter()
                .filter(|(_, selectors)| {
                    selectors.iter().any(|selector| {
                        normalized_data_path(selector) == normalized_data_path(column)
                    })
                })
                .map(|(alias, _)| alias.clone())
                .collect::<BTreeSet<_>>();
            (matches.len() == 1)
                .then(|| matches.into_iter().next())
                .flatten()
        })
        .collect::<Vec<_>>();

    let mut label_counts = HashMap::<String, usize>::new();
    for (column, alias) in columns.iter().zip(candidates.iter()) {
        let label = alias.clone().unwrap_or_else(|| column_header(column));
        *label_counts.entry(label).or_default() += 1;
    }

    columns
        .iter()
        .zip(candidates)
        .filter_map(|(column, alias)| {
            let alias = alias?;
            (label_counts.get(&alias) == Some(&1)).then(|| (column.clone(), alias))
        })
        .collect()
}

fn common_object_class(rows: &[Value]) -> Option<&str> {
    let mut classes = rows.iter().map(|row| {
        row.get("Class")
            .or_else(|| row.get("class"))
            .and_then(Value::as_str)
    });
    let class_name = classes.next()??;
    classes
        .all(|candidate| candidate == Some(class_name))
        .then_some(class_name)
}

fn normalized_data_path(path: &str) -> &str {
    path.strip_prefix("data.")
        .or_else(|| path.strip_prefix("json_data."))
        .unwrap_or(path)
}

fn column_header(column: &str) -> String {
    column.strip_prefix("data.").unwrap_or(column).to_string()
}

fn render_dense_cells(values: &[String], widths: &[usize]) -> Vec<String> {
    let wrapped = values
        .iter()
        .zip(widths)
        .map(|(value, width)| wrap_dense_cell(value, *width))
        .collect::<Vec<_>>();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or_default();

    (0..height)
        .map(|line| {
            render_dense_line(
                wrapped
                    .iter()
                    .map(|cell| cell.get(line).map(String::as_str).unwrap_or_default()),
                widths,
            )
        })
        .collect()
}

fn wrap_dense_cell(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    value
        .split('\n')
        .flat_map(|source_line| {
            let mut lines = Vec::new();
            let mut remaining = source_line.trim_start();
            while remaining.chars().count() > width {
                let hard_end = remaining
                    .char_indices()
                    .nth(width)
                    .map(|(index, _)| index)
                    .unwrap_or(remaining.len());
                let prefix = &remaining[..hard_end];
                let split_at = prefix
                    .char_indices()
                    .rev()
                    .find_map(|(index, character)| {
                        (index > 0 && character.is_whitespace()).then_some(index)
                    })
                    .unwrap_or(hard_end);
                lines.push(remaining[..split_at].trim_end().to_string());
                remaining = remaining[split_at..].trim_start();
            }
            lines.push(remaining.to_string());
            lines
        })
        .collect()
}

fn render_dense_line<'a>(values: impl IntoIterator<Item = &'a str>, widths: &[usize]) -> String {
    values
        .into_iter()
        .zip(widths.iter())
        .map(|(value, width)| {
            let padding = width.saturating_sub(value.chars().count());
            format!("{value}{}", " ".repeat(padding))
        })
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
            table.load_style(ASCII_FULL);
        }
        TableStyle::Compact => {
            table.load_style(UTF8_HORIZONTAL_ONLY);
        }
        TableStyle::Markdown => {
            table.load_style(ASCII_MARKDOWN);
        }
        TableStyle::Plain | TableStyle::Dense => {
            table.load_style(NOTHING);
        }
        TableStyle::Rounded => {
            table.load_style(UTF8_FULL.with_rounded_corners());
        }
    }
}

fn apply_table_layout(table: &mut Table, width: &TableWidth, wrap: &TableWrap, columns: usize) {
    apply_table_layout_at_terminal_width(table, width, wrap, columns, terminal_width());
}

fn apply_table_layout_at_terminal_width(
    table: &mut Table,
    width: &TableWidth,
    wrap: &TableWrap,
    columns: usize,
    terminal_width: Option<usize>,
) {
    let arrangement = match wrap {
        TableWrap::Never => ContentArrangement::Disabled,
        TableWrap::Auto | TableWrap::Fixed(_) => match width {
            TableWidth::Full => ContentArrangement::DynamicFullWidth,
            TableWidth::Auto | TableWidth::Fixed(_) => ContentArrangement::Dynamic,
        },
    };
    table.set_content_arrangement(arrangement);

    match width {
        TableWidth::Auto => {
            if !matches!(wrap, TableWrap::Never) {
                set_table_width_from_terminal(table, terminal_width);
            }
        }
        TableWidth::Full => {
            set_table_width_from_terminal(table, terminal_width);
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

fn set_table_width_from_terminal(table: &mut Table, terminal_width: Option<usize>) {
    if let Some(width) = terminal_width.and_then(|width| u16::try_from(width).ok()) {
        table.set_width(width);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serial_test::serial;
    use std::collections::HashMap;

    use super::{
        append_line, apply_table_layout_at_terminal_width, apply_table_style, column_headers,
        render_dense_rows_with_band, render_dense_theme_preview, reset_output, set_pipeline,
        set_render_format, set_semantic_output, take_output, DenseLayout, OutputSnapshot,
        RenderFormat,
    };
    use crate::config::{init_config, AppConfig};
    use crate::models::{OutputColor, TableBands, TableHeaders, TableStyle, TableWidth, TableWrap};
    use comfy_table::Table;
    use hubuum_filter::{OutputEnvelope, PipeStage, ProjectTerm};
    use hubuum_theme::resolve_theme;

    #[test]
    fn automatic_table_width_wraps_long_cells_within_the_terminal() {
        let mut table = Table::new();
        table.set_header(["key", "value", "source", "detail"]);
        apply_table_style(&mut table, &TableStyle::Rounded);
        apply_table_layout_at_terminal_width(
            &mut table,
            &TableWidth::Auto,
            &TableWrap::Auto,
            4,
            Some(60),
        );
        table.add_row([
            "aliases",
            "a long command alias value that should wrap inside this cell instead of widening the table",
            "user file",
            "/tmp/config.toml",
        ]);

        let rendered = table.to_string();

        assert!(rendered.lines().all(|line| line.chars().count() <= 60));
        assert!(rendered.lines().count() > 5);
    }

    #[test]
    fn automatic_dense_table_width_wraps_long_cells_within_the_terminal() {
        let rows = vec![json!({
            "key": "aliases",
            "value": "a long command alias value that should wrap inside this cell instead of widening the table",
            "source": "user file",
            "detail": "/tmp/config.toml",
        })];
        let columns = ["key", "value", "source", "detail"].map(str::to_string);
        let lines = render_dense_rows_with_band(
            &rows,
            &columns,
            TableHeaders::Full,
            DenseLayout::new(&TableWidth::Auto, &TableWrap::Auto, Some(60)),
            |_, line| line,
        );

        assert!(lines.iter().all(|line| line.chars().count() <= 60));
        assert!(lines.len() > 2);
    }

    #[test]
    fn never_wrap_keeps_automatic_tables_unbounded() {
        let mut table = Table::new();
        table.set_header(["key", "value"]);
        apply_table_style(&mut table, &TableStyle::Rounded);
        apply_table_layout_at_terminal_width(
            &mut table,
            &TableWidth::Auto,
            &TableWrap::Never,
            2,
            Some(30),
        );
        table.add_row([
            "aliases",
            "a value that is deliberately wider than thirty columns",
        ]);

        assert!(table
            .to_string()
            .lines()
            .any(|line| line.chars().count() > 30));
    }

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
    fn detail_rendering_expands_padding_for_long_labels() {
        let mut config = AppConfig::default();
        config.output.color = OutputColor::Never;
        config.output.padding = 4;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");
        set_semantic_output(OutputEnvelope::detail(
            json!({"Name": "alice", "Last Sync Succeeded": "now"}),
            vec!["Name".to_string(), "Last Sync Succeeded".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();
        let colon_offsets = rendered
            .lines()
            .map(|line| line.find(':').expect("detail line should contain a colon"))
            .collect::<Vec<_>>();

        assert_eq!(colon_offsets, vec![19, 19]);
    }

    #[test]
    #[serial]
    fn detail_rendering_aligns_multiline_values() {
        let mut config = AppConfig::default();
        config.output.color = OutputColor::Never;
        config.output.padding = 4;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");
        set_semantic_output(OutputEnvelope::detail(
            json!({"diff": "{\n  \"before\": \"580 GB\",\n  \"after\": \"581 GB\"\n}"}),
            vec!["diff".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert_eq!(
            rendered,
            "diff: {\n        \"before\": \"580 GB\",\n        \"after\": \"581 GB\"\n      }\n"
        );
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
        set_pipeline(vec![PipeStage::Columns(vec![
            ProjectTerm::keep("Name").expect("valid selector")
        ])])
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
            ProjectTerm::keep("Name").expect("valid selector"),
            ProjectTerm::keep("data.network.interfaces[*].ipv4").expect("valid selector"),
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
    fn grouped_headers_stack_complete_data_paths() {
        init_config(AppConfig::default()).expect("config should initialize");
        let rows = vec![json!({"Class": "Hosts"})];
        let columns = vec![
            "data.facts.network.default_ipv4.address".to_string(),
            "data.facts.operating_system.major_version".to_string(),
        ];

        let headers = column_headers(&columns, &rows, TableHeaders::Grouped);

        assert_eq!(
            headers,
            vec![
                "facts\nnetwork\ndefault_ipv4\naddress",
                "facts\noperating_system\nmajor_version",
            ]
        );
    }

    #[test]
    #[serial]
    fn configured_class_aliases_override_grouped_headers() {
        let mut config = AppConfig::default();
        config.output.object_list_class_aliases.insert(
            "Hosts".to_string(),
            HashMap::from([
                (
                    "IPv4".to_string(),
                    vec!["data.facts.network.default_ipv4.address".to_string()],
                ),
                (
                    "OS_major".to_string(),
                    vec!["json_data.facts.operating_system.major_version".to_string()],
                ),
            ]),
        );
        init_config(config).expect("config should initialize");
        let rows = vec![json!({"Class": "Hosts"})];
        let columns = vec![
            "data.facts.network.default_ipv4.address".to_string(),
            "data.facts.operating_system.major_version".to_string(),
        ];

        let headers = column_headers(&columns, &rows, TableHeaders::Grouped);

        assert_eq!(headers, vec!["IPv4", "OS_major"]);
    }

    #[test]
    #[serial]
    fn ambiguous_class_aliases_fall_back_to_grouped_headers() {
        let mut config = AppConfig::default();
        config.output.object_list_class_aliases.insert(
            "Hosts".to_string(),
            HashMap::from([(
                "Address".to_string(),
                vec![
                    "data.facts.network.default_ipv4.address".to_string(),
                    "data.facts.network.default_ipv6.address".to_string(),
                ],
            )]),
        );
        init_config(config).expect("config should initialize");
        let rows = vec![json!({"Class": "Hosts"})];
        let columns = vec![
            "data.facts.network.default_ipv4.address".to_string(),
            "data.facts.network.default_ipv6.address".to_string(),
        ];

        let headers = column_headers(&columns, &rows, TableHeaders::Grouped);

        assert_eq!(
            headers,
            vec![
                "facts\nnetwork\ndefault_ipv4\naddress",
                "facts\nnetwork\ndefault_ipv6\naddress",
            ]
        );
    }

    #[test]
    #[serial]
    fn full_headers_ignore_aliases_and_stay_on_one_line() {
        let mut config = AppConfig::default();
        config.output.object_list_class_aliases.insert(
            "Hosts".to_string(),
            HashMap::from([(
                "IPv4".to_string(),
                vec!["data.facts.network.default_ipv4.address".to_string()],
            )]),
        );
        init_config(config).expect("config should initialize");
        let rows = vec![json!({"Class": "Hosts"})];
        let columns = vec!["data.facts.network.default_ipv4.address".to_string()];

        let headers = column_headers(&columns, &rows, TableHeaders::Full);

        assert_eq!(headers, vec!["facts.network.default_ipv4.address"]);
    }

    #[test]
    #[serial]
    fn none_headers_suppress_header_rows_for_table_renderers() {
        for style in [TableStyle::Plain, TableStyle::Dense] {
            let mut config = AppConfig::default();
            config.output.table_style = style.clone();
            config.output.table_headers = TableHeaders::None;
            config.output.table_bands = TableBands::Never;
            init_config(config).expect("config should initialize");
            reset_output().expect("buffer should reset");
            set_semantic_output(OutputEnvelope::rows(
                vec![json!({"Name": "alpha", "Status": "ready"})],
                vec!["Name".to_string(), "Status".to_string()],
            ))
            .expect("semantic output should be set");

            let rendered = take_output().expect("snapshot").render();

            assert_eq!(rendered.lines().count(), 1, "{style}");
            assert!(!rendered.contains("Name"), "{style}");
            assert!(!rendered.contains("Status"), "{style}");
            assert!(rendered.contains("alpha"), "{style}");
            assert!(rendered.contains("ready"), "{style}");
        }
    }

    #[test]
    #[serial]
    fn dense_grouped_headers_render_each_path_component() {
        let mut config = AppConfig::default();
        config.output.table_style = TableStyle::Dense;
        config.output.table_bands = TableBands::Never;
        init_config(config).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({
                "Class": "Hosts",
                "data.facts.operating_system.major_version": "10"
            })],
            vec!["data.facts.operating_system.major_version".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert_eq!(
            rendered.lines().map(str::trim_end).collect::<Vec<_>>(),
            vec!["facts", "operating_system", "major_version", "10"]
        );
    }

    #[test]
    #[serial]
    fn delimited_headers_keep_semantic_column_names() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("buffer should reset");
        set_render_format(RenderFormat::Csv).expect("render format should set");
        set_semantic_output(OutputEnvelope::rows(
            vec![json!({
                "Class": "Hosts",
                "data.facts.operating_system.major_version": "10"
            })],
            vec!["data.facts.operating_system.major_version".to_string()],
        ))
        .expect("semantic output should be set");

        let rendered = take_output().expect("snapshot").render();

        assert_eq!(rendered, "data.facts.operating_system.major_version\n10\n");
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
