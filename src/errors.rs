use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Command not found")]
    CommandNotFound,

    #[error("Error parsing arguments: {0}")]
    ParseError(String),
}
