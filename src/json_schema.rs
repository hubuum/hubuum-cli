pub(crate) fn schema_paths(schema: &serde_json::Value, include_array_items: bool) -> Vec<String> {
    let mut paths = Vec::new();
    collect_schema_paths(schema, "", include_array_items, &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_schema_paths(
    schema: &serde_json::Value,
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
    use super::schema_paths;
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
}
