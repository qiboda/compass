use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::csv_dir;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressState {
    pub name: String,
    pub status: String,
    pub started_at: String,
    pub updated_at: String,
    pub total_items: Option<u64>,
    pub completed_items: u64,
    pub fetched_rows: u64,
    pub current_item: Option<String>,
    pub percent: Option<f64>,
    pub message: String,
    pub output_csv: Option<String>,
    pub error: Option<String>,
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

/// Path of the JSON progress file for a collector name.
pub fn progress_path(name: &str) -> Result<PathBuf> {
    Ok(csv_dir()?.join(format!("{}.progress.json", sanitize_name(name))))
}

/// Read a progress file, returning None when absent or malformed.
pub fn read_progress(name: &str) -> Result<Option<ProgressState>> {
    let path = progress_path(name)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    match serde_json::from_str(&text) {
        Ok(state) => Ok(Some(state)),
        Err(_) => Ok(None),
    }
}

/// Simple JSON progress tracker for long-running collectors.
#[derive(Debug)]
pub struct Progress {
    name: String,
    path: PathBuf,
    total_items: Option<u64>,
    completed_items: u64,
    fetched_rows: u64,
    current_item: Option<String>,
    message: String,
    status: String,
    output_csv: Option<String>,
    error: Option<String>,
    started_at: String,
}

impl Progress {
    pub fn new(
        name: impl Into<String>,
        total_items: Option<u64>,
        output_csv: Option<PathBuf>,
        message: impl Into<String>,
    ) -> Result<Self> {
        let name = name.into();
        let path = progress_path(&name)?;
        let started_at = now_iso();
        let mut progress = Self {
            name,
            path,
            total_items,
            completed_items: 0,
            fetched_rows: 0,
            current_item: None,
            message: message.into(),
            status: "running".to_string(),
            output_csv: output_csv.map(|p| p.display().to_string()),
            error: None,
            started_at,
        };
        progress.write()?;
        Ok(progress)
    }

    pub fn update(
        &mut self,
        completed: Option<u64>,
        fetched_rows: Option<u64>,
        current_item: Option<String>,
        message: Option<String>,
        total_items: Option<u64>,
    ) -> Result<()> {
        if let Some(v) = total_items {
            self.total_items = Some(v);
        }
        if let Some(v) = completed {
            self.completed_items = v;
        }
        if let Some(v) = fetched_rows {
            self.fetched_rows = v;
        }
        if let Some(v) = current_item {
            self.current_item = Some(v);
        }
        if let Some(v) = message {
            self.message = v;
        }
        self.write()
    }

    pub fn finish(&mut self, fetched_rows: Option<u64>, message: &str) -> Result<()> {
        self.status = "completed".to_string();
        if let Some(v) = fetched_rows {
            self.fetched_rows = v;
        }
        if self.total_items.is_some() {
            self.completed_items = self.total_items.unwrap_or(0);
        }
        self.message = message.to_string();
        self.error = None;
        self.write()
    }

    pub fn fail(&mut self, error: &str, message: &str) -> Result<()> {
        self.status = "failed".to_string();
        self.error = Some(error.to_string());
        self.message = message.to_string();
        self.write()
    }

    fn state(&self) -> ProgressState {
        let percent = match (self.total_items, self.completed_items) {
            (Some(total), completed) if total > 0 => {
                Some((completed.min(total) as f64 / total as f64 * 100.0).min(100.0))
            }
            _ => None,
        };
        ProgressState {
            name: self.name.clone(),
            status: self.status.clone(),
            started_at: self.started_at.clone(),
            updated_at: now_iso(),
            total_items: self.total_items,
            completed_items: self.completed_items,
            fetched_rows: self.fetched_rows,
            current_item: self.current_item.clone(),
            percent,
            message: self.message.clone(),
            output_csv: self.output_csv.clone(),
            error: self.error.clone(),
        }
    }

    fn write(&mut self) -> Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let tmp = self
            .path
            .with_extension(format!("tmp{}", std::process::id()));
        let json = serde_json::to_string_pretty(&self.state())?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_roundtrip() {
        let _guard = crate::config::ENV_MUTEX.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("COMPASS_CSV_DIR", dir.path());
        }
        let mut p = Progress::new("test", Some(10), None, "start").unwrap();
        p.update(Some(3), Some(100), Some("2026-01-01".into()), None, None)
            .unwrap();
        p.finish(Some(100), "done").unwrap();
        let state = read_progress("test").unwrap().unwrap();
        assert_eq!(state.status, "completed");
        assert_eq!(state.fetched_rows, 100);
        assert_eq!(state.completed_items, 10);
        unsafe {
            std::env::remove_var("COMPASS_CSV_DIR");
        }
    }
}
