use std::env;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{CollectError, Result};

/// Serialises tests that mutate process-global environment variables.
#[cfg(test)]
pub(crate) static ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const DEFAULT_DOLT_DIR: &str = "/data/compass-data/compass_data";
const DEFAULT_INVESTMENT_DIR: &str = "/data/compass-data/investment_data";
const DEFAULT_CSV_DIR: &str = "/data/compass-data/csv";

/// `[dolt]` section of `~/.config/compass/config.toml` (same field names as
/// `compass-core::model::DoltConfig`, kept independent so the collectors crate
/// has no dependency on compass-core).
///
/// Missing fields fall back to the built-in defaults (via `#[serde(default)]`),
/// matching compass-data's `load_config` semantics.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    /// Dolt data directories (investment_data, compass_data).
    pub dolt: DoltConfig,
}

/// Dolt directory settings from config.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct DoltConfig {
    #[serde(default)]
    /// Directory for the Dolt `investment_data` repository.
    pub investment_data_dir: String,
    #[serde(default)]
    /// Directory for the Dolt `compass_data` repository.
    pub compass_data_dir: String,
}

/// Read `$HOME/.config/compass/config.toml` (the same location as
/// compass-data's `load_config`). A missing file or a parse error yields the
/// default `FileConfig` (a warn is emitted); the file is re-read on every call
/// so test isolation via `HOME` is order-independent.
pub fn load_file_config() -> FileConfig {
    let config_path = env::var("HOME")
        .map(|home| PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| PathBuf::from("~/.config/compass/config.toml"));

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(path = %config_path.display(), error = %e, "failed to parse config, using defaults");
                FileConfig::default()
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            FileConfig::default()
        }
    }
}

/// Resolve the compass_data Dolt directory: env `COMPASS_DATA_DIR` wins, then
/// config.toml `[dolt].compass_data_dir`, then the built-in default.
pub fn dolt_dir() -> PathBuf {
    if let Ok(v) = env::var("COMPASS_DATA_DIR") {
        return PathBuf::from(v);
    }
    let cfg = load_file_config();
    if cfg.dolt.compass_data_dir.is_empty() {
        PathBuf::from(DEFAULT_DOLT_DIR)
    } else {
        PathBuf::from(cfg.dolt.compass_data_dir)
    }
}

/// Resolve the investment_data Dolt directory: env
/// `COMPASS_INVESTMENT_DATA_DIR` wins, then config.toml
/// `[dolt].investment_data_dir`, then the built-in default.
pub fn investment_data_dir() -> PathBuf {
    if let Ok(v) = env::var("COMPASS_INVESTMENT_DATA_DIR") {
        return PathBuf::from(v);
    }
    let cfg = load_file_config();
    if cfg.dolt.investment_data_dir.is_empty() {
        PathBuf::from(DEFAULT_INVESTMENT_DIR)
    } else {
        PathBuf::from(cfg.dolt.investment_data_dir)
    }
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/name_en_mapping.csv")
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

/// Resolve the default proxy-pool state file path.
pub fn default_proxy_state_path() -> Result<PathBuf> {
    Ok(csv_dir()?.join("proxy_pool_state.json"))
}

/// Whether `dir` is an existing Dolt repository (contains a `.dolt` dir).
pub fn dolt_exists(dir: &std::path::Path) -> bool {
    dir.join(".dolt").exists()
}

/// Fail with [`CollectError::MissingRepo`] when `dir` is not a Dolt repo.
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
    fn name_en_mapping_path_resolves_to_crate_data() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_NAME_EN_MAPPING");
        }
        let path = name_en_mapping_path();
        assert!(
            path.ends_with("data/name_en_mapping.csv"),
            "unexpected path: {}",
            path.display()
        );
        assert!(path.exists(), "mapping CSV must exist: {}", path.display());
    }

    #[test]
    fn csv_dir_creates_and_resolves() {
        let _guard = ENV_MUTEX.blocking_lock();
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

    // ==================================================================
    // Issue #336 A2 — config.toml [dolt] requirement tests (RED, ref #336)
    // ==================================================================

    /// Write a config.toml under `$HOME/.config/compass/` (the same location
    /// used by compass-data's `load_config`).
    fn write_config_toml(home: &std::path::Path, content: &str) {
        let cfg_dir = home.join(".config/compass");
        std::fs::create_dir_all(&cfg_dir).expect("create config directory");
        std::fs::write(cfg_dir.join("config.toml"), content).expect("write config.toml");
    }

    /// #336 A2 happy path: `[dolt]` values from $HOME/.config/compass/config.toml
    /// must drive `dolt_dir()` / `investment_data_dir()`.
    #[test]
    fn dolt_dirs_respect_config_toml() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        write_config_toml(
            tmp.path(),
            "[dolt]\ninvestment_data_dir = \"/cfg/inv\"\ncompass_data_dir = \"/cfg/compass\"\n",
        );
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/cfg/compass"),
            "dolt_dir() must come from config.toml [dolt].compass_data_dir"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/cfg/inv"),
            "investment_data_dir() must come from config.toml [dolt].investment_data_dir"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    /// #336 A2: env vars must take precedence over config.toml values.
    #[test]
    fn env_vars_take_precedence_over_config_toml() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::set_var("COMPASS_DATA_DIR", "/env/compass");
            std::env::set_var("COMPASS_INVESTMENT_DATA_DIR", "/env/inv");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        write_config_toml(
            tmp.path(),
            "[dolt]\ninvestment_data_dir = \"/cfg/inv\"\ncompass_data_dir = \"/cfg/compass\"\n",
        );
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/env/compass"),
            "COMPASS_DATA_DIR must override config.toml"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/env/inv"),
            "COMPASS_INVESTMENT_DATA_DIR must override config.toml"
        );
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
            std::env::remove_var("HOME");
        }
    }

    /// #336 A2: a missing config.toml falls back to the built-in defaults.
    #[test]
    fn missing_config_toml_falls_back_to_defaults() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir"); // deliberately no config.toml
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/data/compass-data/compass_data"),
            "missing config must fall back to the default dolt dir"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/data/compass-data/investment_data"),
            "missing config must fall back to the default investment dir"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    /// #336 A2: a broken config.toml warns and falls back to defaults — it must
    /// never panic.
    #[test]
    fn broken_config_toml_warns_and_falls_back_to_defaults() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        write_config_toml(tmp.path(), "this is not valid {{{ toml");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/data/compass-data/compass_data"),
            "broken config must fall back to the default dolt dir"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/data/compass-data/investment_data"),
            "broken config must fall back to the default investment dir"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    // ==================================================================
    // Issue #336 A2 — adversarial tests for config.toml [dolt] (ref #336)
    //
    // Attacks not covered by the requirement tests above: a partial [dolt]
    // section (missing field must fall back to the built-in default), a
    // config file that exists but has no [dolt] section, and a completely
    // empty config file. All rely on the same ENV_MUTEX + temp HOME
    // isolation pattern.
    // ==================================================================

    /// #336 A2: a partial `[dolt]` section must apply the present field and
    /// fall back to the built-in default for the missing one — `serde(default)`
    /// semantics, same as compass-core's DoltConfig.
    #[test]
    fn partial_dolt_section_missing_field_uses_default() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        // Only investment_data_dir is present; compass_data_dir must default.
        write_config_toml(tmp.path(), "[dolt]\ninvestment_data_dir = \"/cfg/inv\"\n");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/cfg/inv"),
            "the present investment_data_dir must be honored from config.toml"
        );
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/data/compass-data/compass_data"),
            "the missing compass_data_dir must fall back to the built-in default"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    /// #336 A2: a config.toml that exists but carries other sections (no
    /// `[dolt]`) must fall back to the built-in defaults — reading another
    /// section's table must never leak into the dolt dirs.
    #[test]
    fn config_without_dolt_section_uses_default() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        write_config_toml(tmp.path(), "[gui]\nlanguage = \"en\"\n");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/data/compass-data/compass_data"),
            "a config without [dolt] must fall back to the default dolt dir"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/data/compass-data/investment_data"),
            "a config without [dolt] must fall back to the default investment dir"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }

    /// #336 A2: a completely empty config.toml is valid TOML with no tables —
    /// the dolt dirs must still come out as defaults (no panic, no None cell).
    #[test]
    fn empty_config_toml_uses_default() {
        let _guard = ENV_MUTEX.blocking_lock();
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
            std::env::remove_var("COMPASS_INVESTMENT_DATA_DIR");
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        write_config_toml(tmp.path(), "");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        assert_eq!(
            dolt_dir(),
            std::path::PathBuf::from("/data/compass-data/compass_data"),
            "an empty config.toml must fall back to the default dolt dir"
        );
        assert_eq!(
            investment_data_dir(),
            std::path::PathBuf::from("/data/compass-data/investment_data"),
            "an empty config.toml must fall back to the default investment dir"
        );
        unsafe {
            std::env::remove_var("HOME");
        }
    }
}
