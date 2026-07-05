use crate::domain::ResolvedObjectRecord;

use super::{DetailRenderable, TableRenderable};

const DATA_PREVIEW_WIDTH: usize = 72;

impl DetailRenderable for ResolvedObjectRecord {
    fn detail_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Name", self.name.clone()),
            ("Description", self.description.clone()),
            ("Namespace", self.namespace.clone()),
            ("Class", self.class.clone()),
            ("Data", human_readable_bytes(self.data_size())),
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

impl ResolvedObjectRecord {
    fn data_size(&self) -> usize {
        self.data
            .as_ref()
            .map_or(0, |value| value.to_string().len())
    }
}

fn human_readable_bytes(size: usize) -> String {
    if size == 1 {
        return "1 byte".to_string();
    }
    if size < 1024 {
        return format!("{size} bytes");
    }

    let units = ["KB", "MB", "GB", "TB"];
    let mut value = size as f64;

    for unit in units {
        value /= 1024.0;
        if value < 1024.0 {
            if (value.fract() - 0.0).abs() < f64::EPSILON {
                return format!("{value:.0} {unit}");
            }
            return format!("{value:.1} {unit}");
        }
    }

    format!("{value:.1} PB")
}

pub(crate) fn data_preview(data: Option<&serde_json::Value>) -> String {
    match data {
        Some(serde_json::Value::Object(object)) => truncate_preview(
            &object
                .iter()
                .map(|(key, value)| format!("{key}={}", preview_value(value)))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Some(value) => truncate_preview(&preview_value(value)),
        None => String::new(),
    }
}

fn preview_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
    }
}

fn truncate_preview(value: &str) -> String {
    if value.chars().count() <= DATA_PREVIEW_WIDTH {
        return value.to_string();
    }

    let keep = DATA_PREVIEW_WIDTH.saturating_sub(3);
    let truncated = value.chars().take(keep).collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::{data_preview, human_readable_bytes};
    use serde_json::json;

    #[test]
    fn human_readable_bytes_formats_small_values() {
        assert_eq!(human_readable_bytes(0), "0 bytes");
        assert_eq!(human_readable_bytes(1), "1 byte");
        assert_eq!(human_readable_bytes(578), "578 bytes");
    }

    #[test]
    fn human_readable_bytes_formats_larger_values() {
        assert_eq!(human_readable_bytes(1024), "1 KB");
        assert_eq!(human_readable_bytes(1536), "1.5 KB");
        assert_eq!(human_readable_bytes(2 * 1024 * 1024), "2 MB");
    }

    #[test]
    fn data_preview_formats_object_as_key_value_pairs() {
        let value = json!({
            "contact": "Entry",
            "cpu_cpuinfo": "8 x Apple M4",
            "enabled": true
        });

        assert_eq!(
            data_preview(Some(&value)),
            "contact=Entry, cpu_cpuinfo=8 x Apple M4, enabled=true"
        );
    }

    #[test]
    fn data_preview_truncates_on_character_boundaries() {
        let value = json!({
            "cpu_cpuinfo": "8 x Apple M4 Pro øøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøøø"
        });

        let preview = data_preview(Some(&value));
        assert!(preview.ends_with("..."));
        assert!(preview.len() > 3);
    }
}
