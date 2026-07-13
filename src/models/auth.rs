use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub hostname: String,
    #[serde(default)]
    pub identity_scope: Option<String>,
    pub username: String,
    pub token: String,
}

#[cfg(test)]
mod tests {
    use serde_json::from_str;

    use super::TokenEntry;

    #[test]
    fn legacy_token_entries_default_to_the_local_identity_scope() {
        let entry: TokenEntry =
            from_str(r#"{"hostname":"api.example.com","username":"alice","token":"secret"}"#)
                .expect("legacy token entry should deserialize");

        assert_eq!(entry.identity_scope, None);
    }
}
