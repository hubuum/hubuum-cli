use std::io::Write;

use anstream::AutoStream;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::fmt::Display;
use std::fmt::Write as FmtWrite;
use std::sync::Mutex;

use log::debug;

use crate::errors::AppError;
use crate::theme::{paint, ThemeRole};

static OUTPUT_BUFFER: Lazy<Mutex<OutputBuffer>> = Lazy::new(|| Mutex::new(OutputBuffer::new()));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub next_page_command: Option<String>,
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
    filter: Option<(Regex, bool)>,
    warnings: Vec<String>,
    errors: Vec<String>,
    next_page_command: Option<String>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self::default()
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

    fn set_filter(&mut self, pattern: String, invert: bool) -> Result<(), AppError> {
        let regex = Regex::new(&pattern)?;
        debug!("Setting filter: pattern='{pattern}', invert={invert}");
        self.filter = Some((regex, invert));
        Ok(())
    }

    fn set_next_page_command(&mut self, command: String) {
        self.next_page_command = Some(command);
    }

    fn clear_filter(&mut self) {
        self.filter = None;
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.warnings.clear();
        self.errors.clear();
        self.filter = None;
        self.next_page_command = None;
    }

    fn take_snapshot(&mut self) -> OutputSnapshot {
        let lines = if let Some((regex, invert)) = &self.filter {
            self.lines
                .iter()
                .filter(|line| regex.is_match(line) != *invert)
                .cloned()
                .collect()
        } else {
            self.lines.clone()
        };

        let snapshot = OutputSnapshot {
            lines,
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
            next_page_command: self.next_page_command.clone(),
        };

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
    let json_output = serde_json::to_string_pretty(&value).map_err(|_| AppError::FormatError)?;

    let mut output_buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;

    for line in json_output.lines() {
        output_buffer.append_line(line.to_string());
    }

    Ok(())
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
    Ok(OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .take_snapshot())
}

pub fn set_filter(pattern: String, invert: bool) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_filter(pattern, invert)
}

pub fn clear_filter() -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .clear_filter();
    Ok(())
}

pub fn set_next_page_command(command: String) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_next_page_command(command);
    Ok(())
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::{append_line, reset_output, set_filter, take_output};
    use crate::config::{init_config, AppConfig};
    use crate::models::OutputColor;

    #[test]
    #[serial]
    fn take_output_applies_filter_and_resets_buffer() {
        reset_output().expect("buffer should reset");
        append_line("alpha").expect("line should append");
        append_line("beta").expect("line should append");
        set_filter("^b".to_string(), false).expect("filter should set");

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
}
