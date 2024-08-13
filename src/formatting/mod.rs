use std::fmt::Display;

use crate::errors::AppError;
use crate::output::append_line;

mod user;

pub trait OutputFormatter {
    fn format(&self, padding: usize) -> Result<(), AppError>;
}

fn pad_key_value<K, V>(key: K, value: V, padding: usize) -> String
where
    K: Display,
    V: Display,
{
    format!("{:padding$}{}: {}", padding, key, value)
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
