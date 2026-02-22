use log::trace;

use std::collections::HashMap;

use crate::commands::CliOption;
use crate::errors::AppError;

#[derive(Debug)]
pub struct CommandTokenizer {
    scopes: Vec<String>,
    command: String,
    options: HashMap<String, String>,
    positionals: Vec<String>,
}

impl CommandTokenizer {
    pub fn new(input: &str, cmd_name: &str, option_defs: &[CliOption]) -> Result<Self, AppError> {
        let tokens = shlex::split(input).ok_or(AppError::InvalidInput)?;
        let option_lookup = Self::build_option_lookup(option_defs);
        let mut tokenizer = CommandTokenizer {
            scopes: Vec::new(),
            command: String::new(),
            options: HashMap::new(),
            positionals: Vec::new(),
        };

        trace!("Tokenizer generated: {tokens:?}");

        let mut idx = 0;
        let mut seen_options = false;

        while idx < tokens.len() {
            let token = &tokens[idx];

            if !seen_options && token == cmd_name {
                tokenizer.command.clone_from(token);
                idx += 1;
                continue;
            }

            if token.starts_with('-') {
                if tokenizer.command.is_empty() {
                    return Err(AppError::InvalidInput);
                }
                let consumed = tokenizer.parse_option(&tokens, idx, &option_lookup)?;
                idx += consumed;
                seen_options = true;
                continue;
            }

            if seen_options {
                return Err(AppError::InvalidInput);
            }

            if tokenizer.command.is_empty() {
                tokenizer.scopes.push(token.clone());
            } else {
                tokenizer.positionals.push(token.clone());
            }
            idx += 1;
        }

        Ok(tokenizer)
    }

    fn build_option_lookup(option_defs: &[CliOption]) -> HashMap<String, bool> {
        let mut lookup = HashMap::new();

        for opt in option_defs {
            if let Some(short) = opt.short_without_dash() {
                lookup.insert(short, opt.flag);
            }
            if let Some(long) = opt.long_without_dashes() {
                lookup.insert(long, opt.flag);
            }
        }

        lookup
    }

    fn split_option_token(token: &str) -> Result<(String, Option<String>), AppError> {
        let stripped = if let Some(stripped) = token.strip_prefix("--") {
            stripped
        } else if let Some(stripped) = token.strip_prefix('-') {
            stripped
        } else {
            return Err(AppError::InvalidInput);
        };

        if stripped.is_empty() {
            return Err(AppError::InvalidInput);
        }

        if let Some((key, value)) = stripped.split_once('=') {
            if key.is_empty() {
                return Err(AppError::InvalidInput);
            }
            return Ok((key.to_string(), Some(value.to_string())));
        }

        Ok((stripped.to_string(), None))
    }

    fn looks_like_option(token: &str) -> bool {
        if let Some(stripped) = token.strip_prefix("--") {
            return !stripped.is_empty();
        }

        if let Some(stripped) = token.strip_prefix('-') {
            return stripped
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic());
        }

        false
    }

    fn parse_option(
        &mut self,
        tokens: &[String],
        idx: usize,
        option_lookup: &HashMap<String, bool>,
    ) -> Result<usize, AppError> {
        let token = tokens.get(idx).ok_or(AppError::InvalidInput)?;
        let (key, inline_value) = Self::split_option_token(token)?;

        let (value, consumed) = if let Some(value) = inline_value {
            (value, 1)
        } else {
            match option_lookup.get(&key).copied() {
                Some(true) => (String::new(), 1),
                Some(false) => {
                    let next = tokens.get(idx + 1).ok_or_else(|| {
                        AppError::ParseError(format!("Option '--{key}' requires a value"))
                    })?;

                    if Self::looks_like_option(next) {
                        return Err(AppError::ParseError(format!(
                            "Option '--{key}' requires a value"
                        )));
                    }

                    (next.clone(), 2)
                }
                None => {
                    if let Some(next) = tokens.get(idx + 1) {
                        if !Self::looks_like_option(next) {
                            (next.clone(), 2)
                        } else {
                            (String::new(), 1)
                        }
                    } else {
                        (String::new(), 1)
                    }
                }
            }
        };

        self.options
            .insert(key, self.convert_file_and_http_values(&value)?);

        Ok(consumed)
    }

    pub fn convert_file_and_http_values(&self, value: &str) -> Result<String, AppError> {
        let val = if value.starts_with("http://") || value.starts_with("https://") {
            reqwest::blocking::get(value)
                .map_err(|e| AppError::HttpError(e.to_string()))?
                .text()
                .map_err(|e| AppError::HttpError(e.to_string()))?
                .trim_end()
                .to_string()
        } else if let Some(stripped) = value.strip_prefix("file://") {
            std::fs::read_to_string(stripped)
                .map_err(AppError::IoError)?
                .trim_end()
                .to_string()
        } else {
            value.to_string()
        };
        Ok(val)
    }

    #[allow(dead_code)]
    pub fn get_scopes(&self) -> &[String] {
        &self.scopes
    }

    #[allow(dead_code)]
    pub fn get_command(&self) -> Result<&str, AppError> {
        if self.command.is_empty() {
            Err(AppError::CommandNotFound(self.command.clone()))
        } else {
            Ok(&self.command)
        }
    }

    pub fn get_options(&self) -> &HashMap<String, String> {
        &self.options
    }

    pub fn get_positionals(&self) -> &[String] {
        &self.positionals
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::CommandTokenizer;
    use crate::commands::CliOption;
    use crate::errors::AppError;

    fn opt(name: &str, short: Option<&str>, long: Option<&str>, flag: bool) -> CliOption {
        CliOption {
            name: name.to_string(),
            short: short.map(|s| s.to_string()),
            long: long.map(|l| l.to_string()),
            flag,
            help: String::new(),
            field_type: TypeId::of::<String>(),
            field_type_help: "string".to_string(),
            required: false,
            autocomplete: None,
        }
    }

    #[test]
    fn flag_does_not_consume_next_option() {
        let options = vec![
            opt("json", Some("-j"), Some("--json"), true),
            opt("name", Some("-n"), Some("--name"), false),
        ];

        let tokens = CommandTokenizer::new("object list --json --name item-1", "list", &options)
            .expect("tokenization should succeed");

        assert_eq!(tokens.get_options().get("json"), Some(&String::new()));
        assert_eq!(
            tokens.get_options().get("name"),
            Some(&"item-1".to_string())
        );
    }

    #[test]
    fn missing_value_for_non_flag_returns_parse_error() {
        let options = vec![
            opt("json", Some("-j"), Some("--json"), true),
            opt("name", Some("-n"), Some("--name"), false),
        ];

        let err = CommandTokenizer::new("object list --name --json", "list", &options)
            .expect_err("missing option value should fail");

        match err {
            AppError::ParseError(message) => {
                assert!(message.contains("--name"));
                assert!(message.contains("requires a value"));
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn unknown_option_value_is_preserved_for_validation() {
        let options = vec![opt("json", Some("-j"), Some("--json"), true)];

        let tokens =
            CommandTokenizer::new("object list --unknown whatever --json", "list", &options)
                .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_options().get("unknown"),
            Some(&"whatever".to_string())
        );
        assert_eq!(tokens.get_options().get("json"), Some(&String::new()));
    }

    #[test]
    fn parses_inline_value_for_non_flag_option() {
        let options = vec![opt("name", Some("-n"), Some("--name"), false)];

        let tokens = CommandTokenizer::new("object list --name=item-1", "list", &options)
            .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_options().get("name"),
            Some(&"item-1".to_string())
        );
    }

    #[test]
    fn keeps_inline_value_for_flag_option_for_validation() {
        let options = vec![opt("json", Some("-j"), Some("--json"), true)];

        let tokens = CommandTokenizer::new("object list --json=true", "list", &options)
            .expect("tokenization should succeed");

        assert_eq!(tokens.get_options().get("json"), Some(&"true".to_string()));
    }

    #[test]
    fn accepts_negative_number_as_option_value() {
        let options = vec![opt("offset", Some("-o"), Some("--offset"), false)];

        let tokens = CommandTokenizer::new("object list --offset -1", "list", &options)
            .expect("tokenization should succeed");

        assert_eq!(tokens.get_options().get("offset"), Some(&"-1".to_string()));
    }

    #[test]
    fn rejects_non_option_token_after_options_are_seen() {
        let options = vec![opt("json", Some("-j"), Some("--json"), true)];

        let err = CommandTokenizer::new("object list --json trailing", "list", &options)
            .expect_err("non-option token after options should fail");

        assert!(matches!(err, AppError::InvalidInput));
    }

    #[test]
    fn reads_file_uri_option_values() {
        let options = vec![opt("data", Some("-d"), Some("--data"), false)];
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough for tests")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hubuum-cli-tokenizer-{unique_suffix}.txt"));
        fs::write(&path, "payload from file\n").expect("should write test file");

        let line = format!("object list --data file://{}", path.to_string_lossy());
        let tokens =
            CommandTokenizer::new(&line, "list", &options).expect("tokenization should succeed");

        assert_eq!(
            tokens.get_options().get("data"),
            Some(&"payload from file".to_string())
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn unknown_option_without_value_is_empty_string() {
        let options = vec![opt("json", Some("-j"), Some("--json"), true)];

        let tokens = CommandTokenizer::new("object list --unknown --json", "list", &options)
            .expect("tokenization should succeed");

        assert_eq!(tokens.get_options().get("unknown"), Some(&String::new()));
    }

    #[test]
    fn flag_followed_by_free_value_is_invalid_input() {
        let options = vec![opt("json", Some("-j"), Some("--json"), true)];

        let err = CommandTokenizer::new("object list --json true", "list", &options)
            .expect_err("free value after flag should fail tokenization");

        assert!(matches!(err, AppError::InvalidInput));
    }
}
