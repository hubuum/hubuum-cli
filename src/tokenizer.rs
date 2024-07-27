use crate::errors::AppError;
use std::collections::HashMap;

#[derive(Debug)]
pub struct CommandTokenizer {
    scopes: Vec<String>,
    command: String,
    options: HashMap<String, String>,
}

impl CommandTokenizer {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let tokens = shlex::split(input).ok_or(AppError::InvalidInput)?;
        let mut tokenizer = CommandTokenizer {
            scopes: Vec::new(),
            command: String::new(),
            options: HashMap::new(),
        };

        let mut iter = tokens.into_iter();

        // Parse scopes and command
        while let Some(token) = iter.next() {
            if token.starts_with('-') {
                // We've reached the options, so the previous token was the command
                if tokenizer.command.is_empty() {
                    return Err(AppError::InvalidInput);
                }
                tokenizer.parse_options(token, &mut iter)?;
                break;
            } else if tokenizer.command.is_empty() {
                tokenizer.command = token;
            } else {
                tokenizer.scopes.push(token);
            }
        }

        // Parse remaining options
        while let Some(token) = iter.next() {
            tokenizer.parse_options(token, &mut iter)?;
        }

        Ok(tokenizer)
    }

    fn parse_options(
        &mut self,
        key: String,
        iter: &mut std::vec::IntoIter<String>,
    ) -> Result<(), AppError> {
        if key.starts_with("--") {
            let value = iter
                .next()
                .ok_or(AppError::InvalidOption("Option without value".to_string()))?;
            self.options.insert(
                key[2..].to_string(),
                self.convert_file_and_http_values(&value)?,
            );
        } else if key.starts_with('-') {
            let value = iter
                .next()
                .ok_or(AppError::InvalidOption("Option without value".to_string()))?;
            self.options.insert(
                key[1..].to_string(),
                self.convert_file_and_http_values(&value)?,
            );
        } else {
            return Err(AppError::InvalidInput);
        }
        Ok(())
    }

    pub fn convert_file_and_http_values(&self, value: &String) -> Result<String, AppError> {
        let val = if value.starts_with("http://") || value.starts_with("https://") {
            reqwest::blocking::get(value)
                .map_err(|e| AppError::HttpError(e.to_string()))?
                .text()
                .map_err(|e| AppError::HttpError(e.to_string()))?
                .trim_end()
                .to_string()
        } else if value.starts_with("file://") {
            std::fs::read_to_string(&value[7..])
                .map_err(|e| AppError::IoError(e.to_string()))?
                .trim_end()
                .to_string()
        } else {
            value.clone()
        };
        Ok(val)
    }

    pub fn get_scopes(&self) -> &[String] {
        &self.scopes
    }

    pub fn get_command(&self) -> &str {
        &self.command
    }

    pub fn get_options(&self) -> &HashMap<String, String> {
        &self.options
    }
}
