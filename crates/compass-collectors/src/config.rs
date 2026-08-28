use std::env;
use std::path::PathBuf;

use crate::error::{CollectError, Result};

/// Serialises tests that mutate process-global environment variables.
#[cfg(test)]
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const DEFAULT_DOLT_DIR: &str = "/data/compass-data/compass_data";
const DEFAULT_INVESTMENT_DIR: &str = "/data/compass-data/investment_data";
const DEFAULT_CSV_DIR: &str = "/data/compass-data/csv";

/// Resolve the compass_data Dolt directory (env `COMPASS_DATA_DIR` override).
pub fn dolt_dir() -> PathBuf {
    PathBuf::from(env::var("COMPASS_DATA_DIR").unwrap_or_else(|_| DEFAULT_DOLT_DIR.to_string()))
}

/// Resolve the investment_data Dolt directory (env `COMPASS_INVESTMENT_DATA_DIR` override).
pub fn investment_data_dir() -> PathBuf {
    PathBuf::from(
        env::var("COMPASS_INVESTMENT_DATA_DIR")
            .unwrap_or_else(|_| DEFAULT_INVESTMENT_DIR.to_string()),
    )
}

/// Resolve the raw CSV output directory (env `COMPASS_CSV_DIR` override),
/// creating it if absent.
pub fn csv_dir() -> Result<PathBuf> {
    let path =
        PathBuf::from(env::var("COMPASS_CSV_DIR").unwrap_or_else(|_| DEFAULT_CSV_DIR.to_string()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Resolve the name-en mapping CSV path (env `COMPASS_NAME_EN_MAPPING` override).
pub fn name_en_mapping_path() -> PathBuf {
    if let Ok(v) = env::var("COMPASS_NAME_EN_MAPPING") {
        PathBuf::from(v)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("collectors/name_en_mapping.csv")
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|v| v.to_lowercase())
        .is_some_and(|v| v == "1" || v == "true")
}

/// Whether the proxy layer is enabled (`COMPASS_PROXY_DISABLE=1` disables).
pub fn proxy_enabled() -> bool {
    !env_flag("COMPASS_PROXY_DISABLE")
}

pub fn default_proxy_state_path() -> Result<PathBuf> {
    Ok(csv_dir()?.join("proxy_pool_state.json"))
}

pub fn dolt_exists(dir: &std::path::Path) -> bool {
    dir.join(".dolt").exists()
}

pub fn ensure_dolt_repo(dir: &std::path::Path) -> Result<()> {
    if dolt_exists(dir) {
        Ok(())
    } else {
        Err(CollectError::MissingRepo(dir.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_dir_creates_and_resolves() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("COMPASS_CSV_DIR", dir.path());
        }
        let resolved = csv_dir().unwrap();
        assert_eq!(resolved, dir.path());
        assert!(resolved.exists());
        unsafe {
            std::env::remove_var("COMPASS_CSV_DIR");
        }
    }
}
