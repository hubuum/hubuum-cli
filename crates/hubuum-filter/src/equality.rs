use serde_json::{Number, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum JsonEqualityKey {
    Null,
    Boolean(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

pub(crate) fn json_equality_key(value: &Value) -> JsonEqualityKey {
    match value {
        Value::Null => JsonEqualityKey::Null,
        Value::Bool(value) => JsonEqualityKey::Boolean(*value),
        Value::Number(value) => JsonEqualityKey::Number(value.clone()),
        Value::String(value) => JsonEqualityKey::String(value.clone()),
        Value::Array(values) => {
            JsonEqualityKey::Array(values.iter().map(json_equality_key).collect())
        }
        Value::Object(values) => {
            let mut fields = values
                .iter()
                .map(|(name, value)| (name.clone(), json_equality_key(value)))
                .collect::<Vec<_>>();
            fields.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            JsonEqualityKey::Object(fields)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value};

    use super::json_equality_key;

    #[test]
    fn object_field_order_does_not_change_json_equality_keys() {
        let mut left = Map::new();
        left.insert("a".to_string(), Value::from(1));
        left.insert("b".to_string(), Value::from(2));
        let mut right = Map::new();
        right.insert("b".to_string(), Value::from(2));
        right.insert("a".to_string(), Value::from(1));

        assert_eq!(
            json_equality_key(&Value::Object(left)),
            json_equality_key(&Value::Object(right))
        );
    }
}
