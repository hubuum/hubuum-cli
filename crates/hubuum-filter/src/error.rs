use regex::Error as RegexError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Pipe error: {0}")]
    Pipe(String),

    #[error("Pipeline parse error: {0}")]
    Parse(String),

    #[error("Regular expression error: {0}")]
    Regex(#[from] RegexError),

    #[error("JQ error: {0}")]
    Jq(String),
}
