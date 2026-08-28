use std::path::PathBuf;

/// Unified error type for the collector crate.
#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    #[error("HTTP error: {0}")]
    Http(#[from] wreq::Error),

    #[error("HTTP status error: {0}")]
    HttpStatus(u16),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Dolt command failed: {stderr}")]
    Dolt { stderr: String },

    #[error("invalid date {value:?} for {label:?} (expected YYYY-MM-DD)")]
    InvalidDate { label: String, value: String },

    #[error("inverted date range: start {start:?} after end {end:?}")]
    InvertedRange { start: String, end: String },

    #[error("Dolt repo missing: {0}")]
    MissingRepo(PathBuf),

    #[error("empty trading calendar in requested range; refusing to auto-heal without a calendar")]
    EmptyCalendar,

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, CollectError>;
