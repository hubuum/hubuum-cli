use std::fmt::Display;

use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS,
    presets::{ASCII_FULL, ASCII_MARKDOWN, UTF8_FULL, UTF8_HORIZONTAL_ONLY},
    ContentArrangement, Table,
};
use serde::Serialize;

use crate::{
    config::get_config,
    domain::{
        ClassRecord, GroupPermissionsSummary, GroupRecord, NamespaceRecord, ResolvedClassRelationRecord,
        ResolvedObjectRecord, ResolvedObjectRelationRecord, UserRecord,
    },
    errors::AppError,
    models::TableStyle,
    output::append_line,
};

pub trait OutputFormatter: Sized + Serialize + Clone {
    fn format(&self) -> Result<Self, AppError>;

    fn format_noreturn(&self) -> Result<(), AppError> {
        self.format()?;
        Ok(())
    }

    fn format_json_noreturn(&self) -> Result<(), AppError> {
        append_json(self)?;
        Ok(())
    }
}

pub trait DetailRenderable {
    fn detail_rows(&self) -> Vec<(&'static str, String)>;
}

pub trait TableRenderable {
    fn headers() -> Vec<&'static str>;
    fn row(&self) -> Vec<String>;
}

impl<T> OutputFormatter for T
where
    T: DetailRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        let padding = get_config().output.padding;
        for (key, value) in self.detail_rows() {
            append_key_value(key, value, padding)?;
        }
        Ok(self.clone())
    }
}

impl<T> OutputFormatter for Vec<T>
where
    T: TableRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        if self.is_empty() {
            append_line("No results.")?;
            return Ok(self.clone());
        }

        for line in render_table(self)?.lines() {
            append_line(line)?;
        }

        Ok(self.clone())
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

pub fn append_key_value<K, V>(key: K, value: V, padding: i8) -> Result<(), AppError>
where
    K: Display,
    V: Display,
{
    append_line(pad_key_value(key, value, padding))
}

fn pad_key_value<K, V>(key: K, value: V, padding: i8) -> String
where
    K: Display,
    V: Display,
{
    let padding = padding as usize;
    format!("{key:<padding$}: {value}")
}

fn render_table<T>(rows: &[T]) -> Result<String, AppError>
where
    T: TableRenderable,
{
    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(T::headers());

    apply_table_style(&mut table, &get_config().output.table_style);

    for row in rows {
        table.add_row(row.row());
    }

    Ok(table.to_string())
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
        TableStyle::Rounded => {
            table.load_preset(UTF8_FULL);
            table.apply_modifier(UTF8_ROUND_CORNERS);
        }
    }
}

impl DetailRenderable for ClassRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let class = &self.0;
        let schema = schema_label(class.json_schema.as_ref());

        vec![
            ("Name", class.name.clone()),
            ("Description", class.description.clone()),
            ("Namespace", class.namespace.name.clone()),
            ("Schema", schema),
            (
                "Validate",
                class
                    .validate_schema
                    .map_or_else(|| "<none>".to_string(), |value| value.to_string()),
            ),
            ("Created", class.created_at.to_string()),
            ("Updated", class.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ClassRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Namespace",
            "Schema",
            "Validate",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        let class = &self.0;
        vec![
            class.id.to_string(),
            class.name.clone(),
            class.description.clone(),
            class.namespace.name.clone(),
            schema_label(class.json_schema.as_ref()),
            class
                .validate_schema
                .map_or_else(|| "<none>".to_string(), |value| value.to_string()),
            class.created_at.to_string(),
            class.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for GroupRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let group = &self.0;
        vec![
            ("Name", group.groupname.clone()),
            ("Description", group.description.clone()),
            ("Created", group.created_at.to_string()),
            ("Updated", group.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for GroupRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Description", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let group = &self.0;
        vec![
            group.id.to_string(),
            group.groupname.clone(),
            group.description.clone(),
            group.created_at.to_string(),
            group.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for NamespaceRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let namespace = &self.0;
        vec![
            ("Name", namespace.name.clone()),
            ("Description", namespace.description.clone()),
            ("Created", namespace.created_at.to_string()),
            ("Updated", namespace.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for NamespaceRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Name", "Description", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let namespace = &self.0;
        vec![
            namespace.id.to_string(),
            namespace.name.clone(),
            namespace.description.clone(),
            namespace.created_at.to_string(),
            namespace.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for UserRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        let user = &self.0;
        vec![
            ("Username", user.username.clone()),
            (
                "Email",
                user.email.clone().unwrap_or_else(|| "<none>".to_string()),
            ),
            ("Created", user.created_at.to_string()),
            ("Updated", user.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for UserRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "Username", "Email", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        let user = &self.0;
        vec![
            user.id.to_string(),
            user.username.clone(),
            user.email.clone().unwrap_or_default(),
            user.created_at.to_string(),
            user.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedObjectRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Class", self.class.clone()),
            ("Data", self.data_size().to_string()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "Name",
            "Description",
            "Namespace",
            "Class",
            "Data",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.name.clone(),
            self.description.clone(),
            self.namespace.clone(),
            self.class.clone(),
            data_preview(self.data.as_ref()),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedClassRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassFrom", self.from_class.clone()),
            ("ClassTo", self.to_class.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedClassRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec!["id", "FromClass", "ToClass", "Created", "Updated"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.from_class.clone(),
            self.to_class.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl DetailRenderable for ResolvedObjectRelationRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ClassFrom", self.from_class.clone()),
            ("ClassTo", self.to_class.clone()),
            ("ObjectFrom", self.from_object.clone()),
            ("ObjectTo", self.to_object.clone()),
            ("Created", self.created_at.to_string()),
            ("Updated", self.updated_at.to_string()),
        ]
    }
}

impl TableRenderable for ResolvedObjectRelationRecord {
    fn headers() -> Vec<&'static str> {
        vec![
            "id",
            "FromClass",
            "ToClass",
            "FromObject",
            "ToObject",
            "Created",
            "Updated",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.id.to_string(),
            self.from_class.clone(),
            self.to_class.clone(),
            self.from_object.clone(),
            self.to_object.clone(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
        ]
    }
}

impl TableRenderable for GroupPermissionsSummary {
    fn headers() -> Vec<&'static str> {
        vec![
            "Group",
            "Namespace",
            "Class",
            "Object",
            "Class Relation",
            "Object Relation",
        ]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.group.clone(),
            self.namespace.clone(),
            self.class.clone(),
            self.object.clone(),
            self.class_relation.clone(),
            self.object_relation.clone(),
        ]
    }
}

impl ResolvedObjectRecord {
    fn data_size(&self) -> usize {
        self.data
            .as_ref()
            .map_or(0, |value| value.to_string().len())
    }
}

fn schema_label(schema: Option<&serde_json::Value>) -> String {
    let schema_id = schema
        .and_then(|value| value.as_object())
        .and_then(|value| value.get("$id"))
        .and_then(|value| value.as_str());

    match (schema, schema_id) {
        (_, Some(id)) => id.to_string(),
        (Some(_), None) => "<schema without $id>".to_string(),
        (None, _) => "<no schema>".to_string(),
    }
}

fn data_preview(data: Option<&serde_json::Value>) -> String {
    match data {
        Some(value) => {
            let compact = value.to_string();
            if compact.len() > 48 {
                format!("{}...", &compact[..45])
            } else {
                compact
            }
        }
        None => String::new(),
    }
}
