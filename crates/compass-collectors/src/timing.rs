//! Structured timing events for the daily sync pipeline.
//!
//! `compass-collectors sync` writes one JSON line per fetch/import phase to
//! the file named by `COMPASS_TIMING_FILE`. The shell wrapper
//! (`scripts/update-database.sh`) appends its own step events to the same file
//! and merges everything into a single per-run JSON report.
//!
//! Timing is an opportunistic, non-blocking capability: write failures are
//! reported by the caller as warnings and must never abort the data pipeline.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One structured timing event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingEvent {
    /// Event category, currently always `"collector"`.
    pub kind: String,
    /// Collector/source name, e.g. `stock_basic` or `index_daily`.
    pub source: String,
    /// Phase within the source, e.g. `fetch` or `import`.
    pub phase: String,
    /// `"success"` or `"failed"`.
    pub status: String,
    /// Elapsed wall time in milliseconds.
    pub duration_ms: u64,
}

impl TimingEvent {
    /// Create a collector timing event.
    pub fn collector(
        source: impl Into<String>,
        phase: impl Into<String>,
        status: impl Into<String>,
        duration: std::time::Duration,
    ) -> Self {
        Self {
            kind: "collector".to_string(),
            source: source.into(),
            phase: phase.into(),
            status: status.into(),
            duration_ms: duration.as_millis() as u64,
        }
    }
}

/// Appends timing events as JSON Lines to the configured `COMPASS_TIMING_FILE`.
#[derive(Debug, Clone)]
pub struct TimingWriter {
    path: PathBuf,
}

impl TimingWriter {
    /// Create a writer from the `COMPASS_TIMING_FILE` env var.
    ///
    /// Returns `None` when the variable is unset/empty, which keeps timing fully
    /// opt-in.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var_os("COMPASS_TIMING_FILE")?;
        if raw.is_empty() {
            return None;
        }
        Some(Self {
            path: PathBuf::from(raw),
        })
    }

    /// Create a writer for an explicit path (mainly for tests).
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Append one event as a single JSON line.
    ///
    /// Errors (missing parent dir, permissions, etc.) are returned to the
    /// caller, which decides whether to warn. This method never panics.
    pub fn record(&self, event: &TimingEvent) -> std::io::Result<()> {
        let line = serde_json::to_string(event)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn record_appends_valid_jsonl_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("timing.jsonl");
        let writer = TimingWriter::new(path.clone());

        writer
            .record(&TimingEvent::collector(
                "stock_basic",
                "fetch",
                "success",
                Duration::from_millis(42),
            ))
            .expect("record should succeed");

        let content = std::fs::read_to_string(&path).expect("read timing file");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        let event: TimingEvent = serde_json::from_str(lines[0]).expect("valid JSONL");
        assert_eq!(event.kind, "collector");
        assert_eq!(event.source, "stock_basic");
        assert_eq!(event.phase, "fetch");
        assert_eq!(event.status, "success");
        assert_eq!(event.duration_ms, 42);
    }

    #[test]
    fn record_appends_multiple_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("timing.jsonl");
        let writer = TimingWriter::new(path.clone());

        writer
            .record(&TimingEvent::collector(
                "stock_basic",
                "fetch",
                "success",
                Duration::from_millis(1),
            ))
            .expect("first");
        writer
            .record(&TimingEvent::collector(
                "stock_basic",
                "import",
                "success",
                Duration::from_millis(2),
            ))
            .expect("second");

        let content = std::fs::read_to_string(&path).expect("read timing file");
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn record_error_for_missing_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing").join("timing.jsonl");
        let writer = TimingWriter::new(path);
        let result = writer.record(&TimingEvent::collector(
            "stock_basic",
            "fetch",
            "success",
            Duration::from_millis(1),
        ));
        assert!(result.is_err(), "missing parent dir must produce an error");
    }

    #[test]
    fn event_serde_roundtrip_preserves_quotes_and_unicode() {
        let event = TimingEvent::collector(
            "stock_basic \"quoted\" $HOME `cmd`; A&B 中文",
            "fetch",
            "success",
            Duration::from_millis(1),
        );
        let line = serde_json::to_string(&event).expect("serialize");
        let parsed: TimingEvent = serde_json::from_str(&line).expect("deserialize");
        assert_eq!(parsed, event);
    }
}
