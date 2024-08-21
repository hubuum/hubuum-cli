use std::fmt::Display;
use tabled::{settings::Style, Table, Tabled};

use crate::errors::AppError;
use crate::output::append_line;

mod class;
mod group;
mod namespace;
mod user;

pub trait OutputFormatterWithPadding {
    fn format(&self, padding: usize) -> Result<(), AppError>;
}

pub trait OutputFormatter {
    fn format(&self) -> Result<(), AppError>;
}

impl<T> OutputFormatter for Vec<T>
where
    T: Tabled,
{
    fn format(&self) -> Result<(), AppError> {
        let mut table = Table::new(self);
        table.with(Style::modern_rounded());
        let table = table.to_string();
        for line in table.lines() {
            append_line(line)?;
        }
        Ok(())
    }
}

fn pad_key_value<K, V>(key: K, value: V, padding: usize) -> String
where
    K: Display,
    V: Display,
{
    format!("{:<padding$}: {}", key, value, padding = padding)
}

fn append_key_value<K, V>(key: K, value: V, padding: usize) -> Result<(), AppError>
where
    K: Display,
    V: Display,
{
    append_line(pad_key_value(key, value, padding))
}

fn append_some_key_value<K, V>(key: K, value: &Option<V>, padding: usize) -> Result<(), AppError>
where
    K: Display,
    V: Display,
{
    if let Some(value) = value {
        append_key_value(key, value, padding)
    } else {
        append_key_value(key, "<none>", padding)
    }
}
