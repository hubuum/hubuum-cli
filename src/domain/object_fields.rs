use std::collections::BTreeSet;

use serde_json::Value;

pub const DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT: usize = 100;
pub const DEFAULT_OBJECT_FIELD_DEPTH: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectDataPath {
    display: String,
    pointer: String,
    segments: Vec<String>,
}

impl ObjectDataPath {
    fn root() -> Self {
        Self {
            display: "data".to_string(),
            pointer: String::new(),
            segments: Vec::new(),
        }
    }

    fn property(&self, name: &str) -> Self {
        let mut segments = self.segments.clone();
        segments.push(name.to_string());
        Self {
            display: format!("{}.{}", self.display, name),
            pointer: format!("{}/{}", self.pointer, escape_json_pointer_segment(name)),
            segments,
        }
    }

    fn array_item(&self) -> Self {
        let mut segments = self.segments.clone();
        segments.push("0".to_string());
        Self {
            display: format!("{}[*]", self.display),
            pointer: format!("{}/0", self.pointer),
            segments,
        }
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    fn aggregate_path(&self) -> Option<String> {
        self.segments
            .iter()
            .all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
            })
            .then(|| format!("data.{}", self.segments.join(".")))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservedObjectDataFields {
    json_pointers: Vec<String>,
    aggregate_paths: Vec<String>,
}

impl ObservedObjectDataFields {
    pub fn json_pointers(&self) -> &[String] {
        &self.json_pointers
    }

    pub fn aggregate_paths(&self) -> &[String] {
        &self.aggregate_paths
    }
}

pub fn visit_observed_data_fields<'a>(
    data: impl IntoIterator<Item = &'a Value>,
    max_depth: usize,
    mut visitor: impl FnMut(&ObjectDataPath, &'a Value),
) {
    for value in data {
        visit_value(
            value,
            &ObjectDataPath::root(),
            0,
            max_depth,
            false,
            &mut visitor,
        );
    }
}

pub fn observed_object_data_fields<'a>(
    data: impl IntoIterator<Item = &'a Value>,
    max_depth: usize,
) -> ObservedObjectDataFields {
    let mut pointers = BTreeSet::new();
    let mut aggregate_paths = BTreeSet::new();
    visit_observed_data_fields(data, max_depth, |path, _| {
        pointers.insert(path.pointer().to_string());
        if let Some(aggregate_path) = path.aggregate_path() {
            aggregate_paths.insert(aggregate_path);
        }
    });
    ObservedObjectDataFields {
        json_pointers: pointers.into_iter().collect(),
        aggregate_paths: aggregate_paths.into_iter().collect(),
    }
}

fn visit_value<'a>(
    value: &'a Value,
    path: &ObjectDataPath,
    depth: usize,
    max_depth: usize,
    visit_current: bool,
    visitor: &mut impl FnMut(&ObjectDataPath, &'a Value),
) {
    if visit_current {
        visitor(path, value);
    }

    if depth >= max_depth {
        return;
    }

    match value {
        Value::Object(object) => {
            for (name, value) in object {
                visit_value(
                    value,
                    &path.property(name),
                    depth + 1,
                    max_depth,
                    true,
                    visitor,
                );
            }
        }
        Value::Array(values) => {
            let item_path = path.array_item();
            for value in values {
                visit_value(value, &item_path, depth + 1, max_depth, true, visitor);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn escape_json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::observed_object_data_fields;

    #[test]
    fn observed_paths_are_valid_json_pointers() {
        let data = json!({
            "load": {"one": 1, "five": 5},
            "interfaces": [
                {"ipv4": "192.0.2.1"},
                {"mac/address": "00:11", "~label": "uplink"}
            ]
        });

        assert_eq!(
            observed_object_data_fields([&data], 6).json_pointers(),
            vec![
                "/interfaces".to_string(),
                "/interfaces/0".to_string(),
                "/interfaces/0/ipv4".to_string(),
                "/interfaces/0/mac~1address".to_string(),
                "/interfaces/0/~0label".to_string(),
                "/load".to_string(),
                "/load/five".to_string(),
                "/load/one".to_string(),
            ]
        );
    }

    #[test]
    fn observed_paths_respect_the_depth_limit() {
        let data = json!({"one": {"two": {"three": true}}});

        assert_eq!(
            observed_object_data_fields([&data], 2).json_pointers(),
            vec!["/one".to_string(), "/one/two".to_string()]
        );
    }

    #[test]
    fn observed_fields_expose_aggregate_safe_dotted_paths() {
        let data = json!({
            "interfaces": [{"ipv4": "192.0.2.1"}],
            "metrics": {"load": 1.5},
            "invalid.name": true,
            "invalid/name": true
        });

        assert_eq!(
            observed_object_data_fields([&data], 6).aggregate_paths(),
            [
                "data.interfaces".to_string(),
                "data.interfaces.0".to_string(),
                "data.interfaces.0.ipv4".to_string(),
                "data.metrics".to_string(),
                "data.metrics.load".to_string(),
            ]
        );
    }
}
