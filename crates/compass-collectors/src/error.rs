use std::path::PathBuf;

/// Unified error type for the collector crate.
#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    /// HTTP request/transport error from `wreq`.
    #[error("HTTP error: {0}")]
    Http(#[from] wreq::Error),

    /// Non-success HTTP status code.
    #[error("HTTP status error: {0}")]
    HttpStatus(u16),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// CSV read/write error.
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    /// JSON parse/serialize error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// ZIP archive error.
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// Redis client error.
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// Dolt command failed; `stderr` carries the captured stderr output.
    #[error("Dolt command failed: {stderr}")]
    Dolt {
        /// Captured stderr of the failed Dolt command.
        stderr: String,
    },

    /// A date value is not in the expected YYYY-MM-DD format.
    #[error("invalid date {value:?} for {label:?} (expected YYYY-MM-DD)")]
    InvalidDate {
        /// Label of the invalid date (e.g. the calling context).
        label: String,
        /// The offending date value.
        value: String,
    },

    /// A date range has start after end.
    #[error("inverted date range: start {start:?} after end {end:?}")]
    InvertedRange {
        /// Start of the date range.
        start: String,
        /// End of the date range.
        end: String,
    },

    /// The expected Dolt repository directory is missing.
    #[error("Dolt repo missing: {0}")]
    MissingRepo(PathBuf),

    /// The trading calendar is empty for the requested range.
    #[error("empty trading calendar in requested range; refusing to auto-heal without a calendar")]
    EmptyCalendar,

    /// Generic invalid-input error.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Result alias with [`CollectError`] as the error type.
pub type Result<T> = std::result::Result<T, CollectError>;
