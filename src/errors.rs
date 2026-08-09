use std::io::Error as StdIoError;
use std::num::ParseIntError;
use std::str::ParseBoolError;

use config::ConfigError;
use hubuum_client::ApiError;
use hubuum_filter::PipelineError as FilterPipelineError;
use jqesque::JqesqueError;
use regex::Error as RegexError;
use serde_json::Error as JsonError;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReauthenticationRetry {
    Safe,
    Unsafe,
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Failed to execute command: {0}")]
    CommandExecutionError(String),

    #[error("Error parsing arguments: {0}")]
    ParseError(String),

    #[error("Invalid input")]
    InvalidInput,

    #[error("Invalid option: {0}")]
    InvalidOption(String),

    #[error("Boolean flag options with value: {0:?}")]
    PopulatedFlagOptions(Vec<String>),

    #[error("Integer parse error: {0}")]
    ParseIntError(#[from] ParseIntError),

    #[error("JSON parse error: {0}")]
    ParseJsonError(#[from] JsonError),

    #[error("Boolean parse error: {0}")]
    ParseBoolError(#[from] ParseBoolError),

    #[error("Missing required options: {0:?}")]
    MissingOptions(Vec<String>),

    #[error("Duplicate options: {0:?}")]
    DuplicateOptions(Vec<String>),

    #[error("IO error: {0:?}")]
    IoError(#[from] StdIoError),

    #[error("HTTP Error: {0}")]
    HttpError(String),

    #[error("Regular expression error: {0}")]
    RegexError(#[from] RegexError),

    #[error(transparent)]
    PipelineError(#[from] FilterPipelineError),

    #[error("File locking error")]
    LockError,

    #[error("Output format error")]
    FormatError,

    #[allow(dead_code)]
    #[error("Error reading configuration file: {0}")]
    ConfigError(String),

    #[error("Failed to initialize configuration: {0}")]
    ConfigurationError(#[from] ConfigError),

    #[error("REPL error: {0}")]
    ReplError(String),

    #[error("Unable to determine data directory: {0}")]
    DataDirError(String),

    #[error("API error: {0}")]
    ApiError(#[from] ApiError),

    #[error("{source}")]
    UnauthorizedCommand {
        retry: ReauthenticationRetry,
        #[source]
        source: Box<AppError>,
    },

    #[error("Session reauthentication failed after {request}: {source}")]
    ReauthenticationFailed {
        request: String,
        #[source]
        source: Box<AppError>,
    },

    #[error(
        "Session renewed after {request}, but the command was not retried because it may have changed server state before authentication failed. Review the current state, then run it again if appropriate."
    )]
    CommandNotRetried { request: String },

    #[allow(dead_code)]
    #[error("Multiple entities found: {0}")]
    MultipleEntitiesFound(String),

    #[allow(dead_code)]
    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    #[allow(dead_code)]
    #[error("Quiet error")]
    Quiet,

    #[error("Jqesque error: {0}")]
    JqesqueError(#[from] JqesqueError),

    #[error("Error parsing JSONPath: {0}")]
    JsonPathError(String),

    #[error("Configuration error: {0}")]
    GeneralConfigError(String),

    #[error("Extension protocol error for {pack} ({command}): {message}")]
    ExtensionProtocol {
        pack: String,
        command: String,
        message: String,
    },

    #[error("Extension {pack} command {command} failed [{code}]: {message}{details}")]
    ExtensionCommand {
        pack: String,
        command: String,
        code: String,
        message: String,
        details: String,
    },
}

impl AppError {
    pub fn for_command(self, retry: ReauthenticationRetry) -> Self {
        if self.is_unauthorized() {
            Self::UnauthorizedCommand {
                retry,
                source: Box::new(self),
            }
        } else {
            self
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.api_error()
            .and_then(ApiError::status)
            .is_some_and(|status| status == reqwest::StatusCode::UNAUTHORIZED)
    }

    pub fn reauthentication_retry(&self) -> Option<ReauthenticationRetry> {
        match self {
            Self::UnauthorizedCommand { retry, .. } => Some(*retry),
            _ => None,
        }
    }

    pub fn unauthorized_request(&self) -> String {
        let Some(error) = self.api_error() else {
            return "an authenticated request".to_string();
        };
        let method = error
            .request_method()
            .map(ToString::to_string)
            .unwrap_or_else(|| "request".to_string());
        let target = error
            .request_url()
            .map(redacted_request_target)
            .unwrap_or_else(|| "the Hubuum API".to_string());
        format!("{method} {target}")
    }

    pub fn api_error(&self) -> Option<&ApiError> {
        match self {
            Self::ApiError(error) => Some(error),
            Self::UnauthorizedCommand { source, .. } => source.api_error(),
            _ => None,
        }
    }
}

fn redacted_request_target(url: &str) -> String {
    reqwest::Url::parse(url).map_or_else(
        |_| url.split('?').next().unwrap_or(url).to_string(),
        |url| url.path().to_string(),
    )
}
