use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FmError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid FM file: {0}")]
    InvalidFile(String),

    #[error("Decompression failed: {0}")]
    DecompressionError(String),

    #[error("Parse error at offset {offset}: {message}")]
    ParseError { offset: usize, message: String },

    #[error("Entry not found in save directory: {0}")]
    MissingEntry(String),

    #[error("Unsupported FM version: {0}")]
    UnsupportedVersion(String),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, FmError>;
