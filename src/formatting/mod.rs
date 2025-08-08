use serde::Serialize;
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

pub use namespace::FormattedGroupPermissions;
pub use object::FormattedObject;
pub use relations::{FormattedClassRelation, FormattedObjectRelation};

pub trait OutputFormatter: Sized + Serialize + Clone {
    fn format(&self) -> Result<Self, AppError>;
    fn format_noreturn(&self) -> Result<(), AppError> {
        self.format()?;
        Ok(())
    }
    #[allow(dead_code)]
    fn format_json(&self) -> Result<Self, AppError> {
        append_json(self)?;
        Ok(self.clone())
    }
    fn format_json_noreturn(&self) -> Result<(), AppError> {
        append_json(self)?;
        Ok(())
    }
}

impl<T> OutputFormatter for Vec<T>
where
    T: Tabled + Clone + Serialize,
{
    fn format(&self) -> Result<Self, AppError> {
        let table = Table::new(self)
            // This should be customizable by the user, including the ability to disable columns
            // table
            .with(tabled::settings::Style::rounded())
            .clone();
        //    .with(Remove::column(Columns::one(0))); // Disable the first column (ID)
        let table = table.to_string();
        for line in table.lines() {
            append_line(line)?;
        }
        Ok(self.clone())
    }

    fn format_noreturn(&self) -> Result<(), AppError> {
        self.format()?;
        Ok(())
    }

    fn format_json(&self) -> Result<Self, AppError> {
        append_json(self)?;
        Ok(self.clone())
    }

    fn format_json_noreturn(&self) -> Result<(), AppError> {
        append_json(self)?;
        Ok(())
    }
}

fn pad_key_value<K, V>(key: K, value: V, padding: i8) -> String
where
    K: Display,
    V: Display,
{
    let padding = padding as usize;
    format!("{key:<padding$}: {value}")
}

fn append_key_value<K, V>(key: K, value: V, padding: i8) -> Result<(), AppError>
where
    K: Display,
    V: Display,
{
    append_line(pad_key_value(key, value, padding))
}

fn append_some_key_value<K, V>(key: K, value: &Option<V>, padding: i8) -> Result<(), AppError>
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
    T: Serialize,
{
    append_line(serde_json::to_string_pretty(value)?)?;
    Ok(())
}
