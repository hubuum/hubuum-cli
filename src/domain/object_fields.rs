use std::collections::BTreeSet;

use hubuum_client::{ClassFieldInventory, FieldPathSegment, FieldSource};

pub const DEFAULT_OBJECT_FIELD_SAMPLE_LIMIT: usize = 100;
pub const DEFAULT_OBJECT_FIELD_DEPTH: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservedObjectDataFields {
    json_pointers: Vec<String>,
    aggregate_paths: Vec<String>,
}

impl ObservedObjectDataFields {
    /// Array completions start at index zero; literal property names remain literal.
    pub fn from_inventory(inventory: &ClassFieldInventory) -> Self {
        let mut pointers = BTreeSet::new();
        let mut aggregates = BTreeSet::new();
        for field in &inventory.fields {
            if field.source != FieldSource::Data || field.path.is_empty() {
                continue;
            }
            let Some(segments) = field
                .path
                .iter()
                .map(|segment| match segment {
                    FieldPathSegment::Property(name) => Some(name.as_str()),
                    FieldPathSegment::ArrayItem => Some("0"),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            pointers.insert(format!(
                "/{}",
                segments
                    .iter()
                    .map(|segment| segment.replace('~', "~0").replace('/', "~1"))
                    .collect::<Vec<_>>()
                    .join("/")
            ));
            if segments.iter().all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
            }) {
                aggregates.insert(format!("data.{}", segments.join(".")));
            }
        }
        Self {
            json_pointers: pointers.into_iter().collect(),
            aggregate_paths: aggregates.into_iter().collect(),
        }
    }

    pub fn json_pointers(&self) -> &[String] {
        &self.json_pointers
    }
    pub fn aggregate_paths(&self) -> &[String] {
        &self.aggregate_paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fields(paths: Vec<Value>) -> ObservedObjectDataFields {
        let fields = paths.into_iter().map(|path| json!({"source":"data", "field":"display", "path":path, "observation_count":1, "object_count":1, "types":{}, "example_truncated":false, "error_count":0})).collect::<Vec<_>>();
        let inventory: ClassFieldInventory = serde_json::from_value(json!({"sampled":true,"sampled_objects":1,"effective_sample_limit":100,"max_depth":6,"include_containers":true,"truncation":[],"fields":fields})).unwrap();
        ObservedObjectDataFields::from_inventory(&inventory)
    }

    #[test]
    fn pointers_preserve_literal_names_and_default_array_indices() {
        let result = fields(vec![
            json!([{"kind":"property","name":"interfaces"},{"kind":"array_item"},{"kind":"property","name":"mac/address~"}]),
            json!([{"kind":"property","name":"[#]"}]),
            json!([{"kind":"property","name":"[0]"}]),
            json!([{"kind":"property","name":""}]),
        ]);
        assert_eq!(
            result.json_pointers(),
            ["/", "/[#]", "/[0]", "/interfaces/0/mac~1address~0"]
        );
    }

    #[test]
    fn aggregate_completions_exclude_unrepresentable_properties() {
        let result = fields(vec![
            json!([{"kind":"property","name":"interfaces"},{"kind":"array_item"},{"kind":"property","name":"ipv4"}]),
            json!([{"kind":"property","name":"invalid.name"}]),
            json!([{"kind":"property","name":"invalid/name"}]),
        ]);
        assert_eq!(result.aggregate_paths(), ["data.interfaces.0.ipv4"]);
    }

    #[test]
    fn unknown_path_segments_are_not_offered_as_valid_completions() {
        assert!(fields(vec![json!([{"kind":"future"}])])
            .json_pointers()
            .is_empty());
    }
}
