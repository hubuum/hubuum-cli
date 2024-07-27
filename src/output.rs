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

pub struct OutputBuffer {
    lines: Vec<String>,
    filter: Option<(Regex, bool)>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl OutputBuffer {
    fn new() -> Self {
        OutputBuffer {
            lines: Vec::new(),
            filter: None,
            warnings: Vec::new(),
            errors: Vec::new(),
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

    fn set_filter(&mut self, pattern: String, invert: bool) -> Result<(), AppError> {
        let regex = Regex::new(&pattern)?;
        debug!("Setting filter: pattern='{}', invert={}", pattern, invert);
        self.filter = Some((regex, invert));
        Ok(())
    }

    fn clear_filter(&mut self) {
        self.filter = None;
    }

    fn flush(&mut self) {
        debug!("Flushing output buffer ({} lines)", self.lines.len());

        for warning in &self.warnings {
            println!("{}", format!("Warning: {}", warning).yellow());
        }
        self.warnings.clear();

        for error in &self.errors {
            println!("{}", format!("Error: {}", error).red());
        }
        self.errors.clear();

        if let Some((regex, invert)) = &self.filter {
            debug!(
                "Filtering output buffer with pattern='{}', invert={}",
                regex, invert
            );
            for line in &self.lines {
                let matches = regex.is_match(line);
                if matches != *invert {
                    println!("{}", line);
                }
            }
        } else {
            for line in &self.lines {
                println!("{}", line);
            }
        }
        self.lines.clear();
    }
}

/// Add a warning message to the output buffer.
///
/// This function adds a warning message that will always be displayed when flushing the output.
///
/// ## Examples
///
/// ```
/// use crate::output::add_warning;
///
/// add_warning("This is a warning message")?;
/// ```
///
/// ## Errors
///
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn add_warning<T: Display>(message: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .add_warning(message.to_string());
    Ok(())
}

/// Add an error message to the output buffer.
///
/// This function adds an error message that will always be displayed when flushing the output.
///
/// ## Examples
///
/// ```
/// use crate::output::add_error;
///
/// add_error("This is an error message")?;
/// ```
///
/// ## Errors
///
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn add_error<T: Display>(message: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .add_error(message.to_string());
    Ok(())
}

/// Append a line to the output buffer.
///
/// This function appends the provided line to the output buffer.
///
/// ## Examples
///
/// ```
/// use crate::output::append_line;
///
/// append_line("Hello, world!")?;
/// ```
///
/// ## Errors
///
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn append_line<T: Display>(line: T) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .append_line(line.to_string());
    Ok(())
}

/// Append multiple lines to the output buffer.
///
/// This function appends each line in the provided slice to the output buffer.
///
/// ## Examples
///
/// ```
/// use crate::output::append_lines;
///
/// let lines = vec!["Line 1", "Line 2", "Line 3"];
/// append_lines(&lines)?;
/// ````
///
/// ## Errors
///
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn append_lines<T: Display>(lines: &[T]) -> Result<(), AppError> {
    let mut buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;
    for line in lines {
        buffer.append_line(line.to_string());
    }
    Ok(())
}

/// Append a debug representation of a value to the output buffer.
///
/// This function uses the `Debug` trait to format the value, line by line into
/// the output buffer.
///
/// ## Examples
///
/// ```
/// use crate:output::append_debug;
///
/// let value = vec![1, 2, 3];
/// append_debug(value)?;
/// ````
///
/// ## Errors
///
///  - OutputError::FormatError if the value cannot be formatted.
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn append_debug<T: std::fmt::Debug>(value: T) -> Result<(), AppError> {
    let mut debug_output = String::new();
    write!(&mut debug_output, "{:#?}", value).map_err(|_| AppError::FormatError)?;

    let mut output_buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;

    for line in debug_output.lines() {
        output_buffer.append_line(line.to_string());
    }

    Ok(())
}

pub fn append_json<T: Serialize>(value: T) -> Result<(), AppError> {
    let json_output = serde_json::to_string_pretty(&value).map_err(|_| AppError::FormatError)?;

    let mut output_buffer = OUTPUT_BUFFER.lock().map_err(|_| AppError::LockError)?;

    for line in json_output.lines() {
        output_buffer.append_line(line.to_string());
    }

    Ok(())
}

/// Flush the output buffer to stdout.
///
/// This function flushes the output buffer to stdout, printing each line in the
/// buffer. If a filter is set, only lines matching the filter will be printed.
///
/// ## Examples
///
/// ```
/// use crate::output::flush_output;
///
/// flush_output()?;
/// ````
///
/// ## Errors
///  - OutputError::LockError if the output buffer cannot be locked.
pub fn flush_output() -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .flush();
    Ok(())
}

/// Set a filter on the output buffer.
///
/// This function sets a regular expression filter on the output buffer. By default,
/// only lines matching the expression will be printed. If the invert flag is set,
/// lines matching the pattern will be excluded from the output.
pub fn set_filter(pattern: String, invert: bool) -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .set_filter(pattern, invert)?;
    Ok(())
}

/// Clear the filter on the output buffer.
pub fn clear_filter() -> Result<(), AppError> {
    OUTPUT_BUFFER
        .lock()
        .map_err(|_| AppError::LockError)?
        .clear_filter();
    Ok(())
}
