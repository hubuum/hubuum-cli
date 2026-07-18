use serde_json::Value;

pub(crate) fn schema_paths(schema: &Value, include_array_items: bool) -> Vec<String> {
    let mut paths = Vec::new();
    collect_schema_paths(schema, "", include_array_items, &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn schema_json_pointers(schema: &Value) -> Vec<String> {
    let mut pointers = Vec::new();
    collect_schema_json_pointers(schema, "", &mut pointers);
    pointers.sort();
    pointers.dedup();
    pointers
}

fn collect_schema_json_pointers(schema: &Value, prefix: &str, pointers: &mut Vec<String>) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };

    for (name, property_schema) in properties {
        let pointer = format!("{prefix}/{}", escape_json_pointer_segment(name));
        pointers.push(pointer.clone());
        collect_schema_json_pointers(property_schema, &pointer, pointers);

        if let Some(items) = property_schema.get("items") {
            let item_pointer = format!("{pointer}/0");
            pointers.push(item_pointer.clone());
            collect_schema_json_pointers(items, &item_pointer, pointers);
        }
    }
}

fn escape_json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn collect_schema_paths(
    schema: &Value,
    prefix: &str,
    include_array_items: bool,
    paths: &mut Vec<String>,
) {
    let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
        return;
    };

    for (name, property_schema) in properties {
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        paths.push(path.clone());
        collect_schema_paths(property_schema, &path, include_array_items, paths);
        if include_array_items {
            if let Some(items) = property_schema.get("items") {
                collect_schema_paths(items, &format!("{path}[*]"), include_array_items, paths);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{schema_json_pointers, schema_paths};
    use serde_json::json;

    #[test]
    fn schema_paths_can_include_array_item_paths() {
        let schema = json!({
            "properties": {
                "network": {
                    "properties": {
                        "interfaces": {
                            "items": {
                                "properties": {
                                    "ipv4": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }
        });

        assert_eq!(
            schema_paths(&schema, true),
            vec![
                "network".to_string(),
                "network.interfaces".to_string(),
                "network.interfaces[*].ipv4".to_string(),
            ]
        );
        assert_eq!(
            schema_paths(&schema, false),
            vec!["network".to_string(), "network.interfaces".to_string()]
        );
    }

    #[test]
    fn schema_paths_can_be_rendered_as_json_pointers() {
        let schema = json!({
            "properties": {
                "network": {
                    "properties": {
                        "interfaces": {
                            "items": {
                                "properties": {
                                    "mac/address": {"type": "string"},
                                    "~label": {"type": "string"}
                                }
                            }
                        }
                    }
                }
            }
        });

        assert_eq!(
            schema_json_pointers(&schema),
            vec![
                "/network".to_string(),
                "/network/interfaces".to_string(),
                "/network/interfaces/0".to_string(),
                "/network/interfaces/0/mac~1address".to_string(),
                "/network/interfaces/0/~0label".to_string(),
            ]
        );
    }
}
