use std::fmt::Display;
use tabled::{Table, Tabled};
// use tabled::settings::{object::Columns, Remove, Style};

use crate::errors::AppError;
use crate::output::append_line;

mod class;
mod group;
mod namespace;
mod object;
mod relations;
mod user;

pub use object::FormattedObject;
pub use relations::{FormattedClassRelation, FormattedObjectRelation};

pub trait OutputFormatterWithPadding: Sized {
    fn format(&self, padding: usize) -> Result<Self, AppError>;
}

pub trait OutputFormatter: Sized {
    fn format(&self) -> Result<Self, AppError>;
}

impl<T> OutputFormatter for Vec<T>
where
    T: Tabled + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        let table = Table::new(self);
        // This should be customizable by the user, including the ability to disable columns
        // table
        //            .with(Style::modern_rounded())
        //    .with(Remove::column(Columns::one(0))); // Disable the first column (ID)
        let table = table.to_string();
        for line in table.lines() {
            append_line(line)?;
        }
        Ok(self.clone())
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
