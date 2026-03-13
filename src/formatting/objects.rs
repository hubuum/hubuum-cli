use crate::domain::ResolvedObjectRecord;

use super::{DetailRenderable, TableRenderable};

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

#[cfg(test)]
mod tests {
    use super::human_readable_bytes;

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
}
