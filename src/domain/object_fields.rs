use std::collections::BTreeSet;

use serde_json::Value;

pub const DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT: usize = 100;
pub const DEFAULT_OBJECT_FIELD_DEPTH: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectDataPath {
    display: String,
    pointer: String,
}

impl ObjectDataPath {
    fn root() -> Self {
        Self {
            display: "data".to_string(),
            pointer: String::new(),
        }
    }

    fn property(&self, name: &str) -> Self {
        Self {
            display: format!("{}.{}", self.display, name),
            pointer: format!("{}/{}", self.pointer, escape_json_pointer_segment(name)),
        }
    }

    fn array_item(&self) -> Self {
        Self {
            display: format!("{}[*]", self.display),
            pointer: format!("{}/0", self.pointer),
        }
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn pointer(&self) -> &str {
        &self.pointer
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

pub fn observed_json_pointers<'a>(
    data: impl IntoIterator<Item = &'a Value>,
    max_depth: usize,
) -> Vec<String> {
    let mut pointers = BTreeSet::new();
    visit_observed_data_fields(data, max_depth, |path, _| {
        pointers.insert(path.pointer().to_string());
    });
    pointers.into_iter().collect()
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

    use super::observed_json_pointers;

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
            observed_json_pointers([&data], 6),
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
            observed_json_pointers([&data], 2),
            vec!["/one".to_string(), "/one/two".to_string()]
        );
    }
}
