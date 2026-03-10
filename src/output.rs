use colored::Colorize;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::fmt::Display;
use std::fmt::Write;
use std::sync::Mutex;

use log::debug;

use crate::errors::AppError;

static OUTPUT_BUFFER: Lazy<Mutex<OutputBuffer>> = Lazy::new(|| Mutex::new(OutputBuffer::new()));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
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
                .map(|warning| format!("Warning: {warning}").yellow().to_string()),
        );
        rendered.extend(
            self.errors
                .iter()
                .map(|error| format!("Error: {error}").red().to_string()),
        );
        rendered.extend(self.lines.iter().cloned());

        if rendered.is_empty() {
            String::new()
        } else {
            format!("{}\n", rendered.join("\n"))
        }
    }
}

#[derive(Debug, Default)]
pub struct OutputBuffer {
    lines: Vec<String>,
    filter: Option<(Regex, bool)>,
    warnings: Vec<String>,
    errors: Vec<String>,
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

    fn clear_filter(&mut self) {
        self.filter = None;
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.warnings.clear();
        self.errors.clear();
        self.filter = None;
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

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::{append_line, reset_output, set_filter, take_output};

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
}
