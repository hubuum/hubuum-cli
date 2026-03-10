use log::trace;

use std::collections::HashMap;

use crate::commands::CliOption;
use crate::errors::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionOccurrence {
    pub key: String,
    pub value: String,
}

#[derive(Debug)]
pub struct CommandTokenizer {
    raw_tokens: Vec<String>,
    scopes: Vec<String>,
    command: String,
    options: HashMap<String, String>,
    option_occurrences: Vec<OptionOccurrence>,
    positionals: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct OptionParseSpec {
    flag: bool,
    greedy: bool,
    nargs: Option<usize>,
}

impl CommandTokenizer {
    pub fn new(input: &str, cmd_name: &str, option_defs: &[CliOption]) -> Result<Self, AppError> {
        let tokens = shlex::split(input).ok_or(AppError::InvalidInput)?;
        let option_lookup = Self::build_option_lookup(option_defs);
        let mut tokenizer = CommandTokenizer {
            raw_tokens: tokens.clone(),
            scopes: Vec::new(),
            command: String::new(),
            options: HashMap::new(),
            option_occurrences: Vec::new(),
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

    fn build_option_lookup(option_defs: &[CliOption]) -> HashMap<String, OptionParseSpec> {
        let mut lookup = HashMap::new();

        for opt in option_defs {
            let spec = OptionParseSpec {
                flag: opt.flag,
                greedy: opt.greedy,
                nargs: opt.nargs,
            };
            if let Some(short) = opt.short_without_dash() {
                lookup.insert(short, spec);
            }
            if let Some(long) = opt.long_without_dashes() {
                lookup.insert(long, spec);
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
        option_lookup: &HashMap<String, OptionParseSpec>,
    ) -> Result<usize, AppError> {
        let token = tokens.get(idx).ok_or(AppError::InvalidInput)?;
        let (key, inline_value) = Self::split_option_token(token)?;

        let parse_spec = option_lookup.get(&key).copied().unwrap_or(OptionParseSpec {
            flag: false,
            greedy: false,
            nargs: None,
        });

        let (value, consumed) = if let Some(value) = inline_value {
            if let Some(nargs) = parse_spec.nargs {
                self.consume_fixed_arity_value(tokens, idx, value, nargs, option_lookup)?
            } else if parse_spec.greedy {
                self.consume_greedy_value(tokens, idx, value, option_lookup)?
            } else {
                (value, 1)
            }
        } else {
            match option_lookup.get(&key).copied() {
                Some(spec) if spec.flag => (String::new(), 1),
                Some(spec) if spec.nargs.is_some() => self.consume_fixed_arity_value(
                    tokens,
                    idx,
                    String::new(),
                    spec.nargs.expect("nargs checked above"),
                    option_lookup,
                )?,
                Some(spec) if spec.greedy => {
                    self.consume_greedy_value(tokens, idx, String::new(), option_lookup)?
                }
                Some(_) => {
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
        self.option_occurrences.push(OptionOccurrence {
            key: token_key(token),
            value: self.convert_file_and_http_values(&value)?,
        });

        Ok(consumed)
    }

    fn consume_greedy_value(
        &self,
        tokens: &[String],
        idx: usize,
        initial: String,
        option_lookup: &HashMap<String, OptionParseSpec>,
    ) -> Result<(String, usize), AppError> {
        let mut values = Vec::new();
        if !initial.is_empty() {
            values.push(initial);
        }

        let mut consumed = 1;
        while let Some(next) = tokens.get(idx + consumed) {
            if Self::is_known_option(next, option_lookup) {
                break;
            }
            values.push(next.clone());
            consumed += 1;
        }

        if values.is_empty() {
            let key = tokens
                .get(idx)
                .map(|token| {
                    token
                        .trim_start_matches('-')
                        .split('=')
                        .next()
                        .unwrap_or(token)
                })
                .unwrap_or("option");
            return Err(AppError::ParseError(format!(
                "Option '--{key}' requires a value"
            )));
        }

        Ok((values.join(" "), consumed))
    }

    fn consume_fixed_arity_value(
        &self,
        tokens: &[String],
        idx: usize,
        initial: String,
        nargs: usize,
        option_lookup: &HashMap<String, OptionParseSpec>,
    ) -> Result<(String, usize), AppError> {
        let mut values = Vec::new();
        if !initial.is_empty() {
            values.push(initial);
        }

        let mut consumed = 1;
        while values.len() < nargs {
            let Some(next) = tokens.get(idx + consumed) else {
                return Err(self.fixed_arity_error(tokens, idx, nargs));
            };

            if Self::is_known_option(next, option_lookup) {
                return Err(self.fixed_arity_error(tokens, idx, nargs));
            }

            values.push(next.clone());
            consumed += 1;
        }

        Ok((values.join(" "), consumed))
    }

    fn fixed_arity_error(&self, tokens: &[String], idx: usize, nargs: usize) -> AppError {
        let key = tokens
            .get(idx)
            .map(|token| {
                token
                    .trim_start_matches('-')
                    .split('=')
                    .next()
                    .unwrap_or(token)
            })
            .unwrap_or("option");
        AppError::ParseError(format!("Option '--{key}' requires {nargs} value elements"))
    }

    fn is_known_option(token: &str, option_lookup: &HashMap<String, OptionParseSpec>) -> bool {
        let Ok((key, _value)) = Self::split_option_token(token) else {
            return false;
        };
        option_lookup.contains_key(&key)
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

    pub fn get_option_occurrences(&self) -> &[OptionOccurrence] {
        &self.option_occurrences
    }

    #[cfg(test)]
    pub fn get_option_values(&self, key: &str) -> Vec<String> {
        self.option_occurrences
            .iter()
            .filter(|occurrence| occurrence.key == key)
            .map(|occurrence| occurrence.value.clone())
            .collect()
    }

    pub fn raw_tokens(&self) -> &[String] {
        &self.raw_tokens
    }

    pub fn get_positionals(&self) -> &[String] {
        &self.positionals
    }
}

fn token_key(token: &str) -> String {
    token
        .trim_start_matches('-')
        .split('=')
        .next()
        .unwrap_or(token)
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::CommandTokenizer;
    use crate::commands::CliOption;
    use crate::errors::AppError;

    fn opt(name: &str, short: Option<&str>, long: Option<&str>, flag: bool) -> CliOption {
        CliOption {
            name: name.to_string(),
            short: short.map(|s| s.to_string()),
            long: long.map(|l| l.to_string()),
            flag,
            greedy: false,
            nargs: None,
            repeatable: false,
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
    fn repeated_option_occurrences_are_preserved_in_order() {
        let options = vec![opt("where", None, Some("--where"), false)];

        let tokens = CommandTokenizer::new(
            "class list --where 'name icontains foo' --where 'description contains bar'",
            "list",
            &options,
        )
        .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_option_values("where"),
            vec![
                "name icontains foo".to_string(),
                "description contains bar".to_string()
            ]
        );
        assert_eq!(
            tokens
                .get_options()
                .get("where")
                .expect("last value should be preserved in flat view"),
            "description contains bar"
        );
    }

    #[test]
    fn fixed_arity_option_consumes_exactly_three_elements() {
        let mut where_opt = opt("where", None, Some("--where"), false);
        where_opt.nargs = Some(3);
        let options = vec![where_opt, opt("limit", None, Some("--limit"), false)];

        let tokens = CommandTokenizer::new(
            "namespace list --where description icontains foo --limit 10",
            "list",
            &options,
        )
        .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_option_values("where"),
            vec!["description icontains foo".to_string()]
        );
        assert_eq!(tokens.get_options().get("limit"), Some(&"10".to_string()));
    }

    #[test]
    fn fixed_arity_option_requires_exact_value_count() {
        let mut where_opt = opt("where", None, Some("--where"), false);
        where_opt.nargs = Some(3);
        let options = vec![where_opt, opt("json", Some("-j"), Some("--json"), true)];

        let err = CommandTokenizer::new("namespace list --where --json", "list", &options)
            .expect_err("missing fixed-arity value should fail");

        match err {
            AppError::ParseError(message) => {
                assert!(message.contains("--where"));
                assert!(message.contains("requires 3 value elements"));
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn repeated_fixed_arity_options_split_on_next_same_option() {
        let mut where_opt = opt("where", None, Some("--where"), false);
        where_opt.nargs = Some(3);
        let options = vec![where_opt];

        let tokens = CommandTokenizer::new(
            "namespace list --where name contains foo --where description contains bar",
            "list",
            &options,
        )
        .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_option_values("where"),
            vec![
                "name contains foo".to_string(),
                "description contains bar".to_string()
            ]
        );
    }

    #[test]
    fn fixed_arity_option_accepts_quoted_values_with_spaces() {
        let mut where_opt = opt("where", None, Some("--where"), false);
        where_opt.nargs = Some(3);
        let options = vec![where_opt];

        let tokens = CommandTokenizer::new(
            "namespace list --where description contains 'bar baz'",
            "list",
            &options,
        )
        .expect("tokenization should succeed");

        assert_eq!(
            tokens.get_option_values("where"),
            vec!["description contains bar baz".to_string()]
        );
    }
}
