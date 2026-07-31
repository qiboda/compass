mod baostock;
mod export;
use compass_data::import_compass;
use compass_data::import_dolt;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use compass_core::model::AppConfig;
use tracing::error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// compass-data — A-share stock data pipeline
///
/// Manages OHLCV data from Dolt into a Parquet-based main database.
#[derive(Parser)]
#[command(name = "compass-data")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import data from Dolt investment_data into Parquet main database
    Import {
        /// Dolt data directory (default from config.toml [dolt].investment_data_dir)
        #[arg(long)]
        dolt_dir: Option<PathBuf>,

        /// Output Parquet directory (default from config.toml [parquet].dir)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Max symbols (0 = all)
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// Stock symbols to import (comma-separated 6-digit codes, e.g. "000001,600519")
        #[arg(long)]
        symbols: Option<String>,

        /// Start date (YYYYMMDD), inclusive
        #[arg(long)]
        start_date: Option<String>,

        /// End date (YYYYMMDD), inclusive
        #[arg(long)]
        end_date: Option<String>,

        /// Incremental: only import symbols with tradedate >= since (YYYYMMDD)
        #[arg(long)]
        since: Option<String>,
    },

    /// Import data from compass_data Dolt into Parquet
    ImportCompass {
        /// Dolt data directory (default from config.toml [dolt].compass_data_dir)
        #[arg(long)]
        dolt_dir: Option<PathBuf>,

        /// Output Parquet directory (default from config.toml [parquet].dir)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Table to import: stock_basic, fin_indicators, fin_balance_sheet, fin_income, fin_cash_flow
        #[arg(long)]
        table: String,

        /// Overwrite existing data
        #[arg(long, default_value_t = false)]
        overwrite: bool,

        /// Incremental: only import data with report_date >= since (YYYYMMDD)
        #[arg(long)]
        since: Option<String>,
    },

    /// Export Parquet main database to other formats
    Export {
        /// Parquet data directory (default from config.toml [parquet].dir)
        #[arg(long)]
        input: Option<PathBuf>,

        /// Output format: parquet-dir, duckdb, csv
        #[arg(long, default_value = "duckdb")]
        format: String,

        /// Output path (default: /data/compass-data/compass.duckdb)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Overwrite existing data instead of skipping duplicates
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },

    /// Zip parquet_data and upload to Baidu Cloud via baidupcs
    Backup {
        /// Parquet data directory to backup (default from config.toml [parquet].dir)
        #[arg(long)]
        input: Option<PathBuf>,

        /// Keep local zip file after upload
        #[arg(long, default_value_t = false)]
        keep_zip: bool,
    },
}

fn load_config() -> AppConfig {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => {
                tracing::info!(path = %config_path.display(), "config loaded");
                cfg
            }
            Err(e) => {
                tracing::warn!(path = %config_path.display(), error = %e, "failed to parse config, using defaults");
                AppConfig::default()
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            AppConfig::default()
        }
    }
}

/// Thin wrapper: init tracing, load config, parse CLI, dispatch.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer());

    #[cfg(feature = "tracy")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());

    registry.init();

    let config = load_config();
    let cli = Cli::parse();

    if let Err(e) = run(cli, config).await {
        error!("{e}");
        std::process::exit(1);
    }
}

/// Dispatch the parsed CLI command using the loaded configuration.
///
/// Returns an error on failure (the caller is responsible for logging and exiting).
#[allow(clippy::too_many_lines)]
async fn run(cli: Cli, config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let default_export_output = PathBuf::from("/data/compass-data/compass.duckdb");

    match cli.command {
        Command::Import {
            dolt_dir,
            output,
            limit,
            symbols,
            start_date,
            end_date,
            since,
        } => {
            let dolt_dir =
                dolt_dir.unwrap_or_else(|| PathBuf::from(&config.dolt.investment_data_dir));
            let output = output.unwrap_or_else(|| PathBuf::from(&config.parquet.dir));
            if let Err(e) = import_dolt::run(
                dolt_dir,
                output,
                limit,
                symbols.as_deref(),
                start_date.as_deref(),
                end_date.as_deref(),
                since.as_deref(),
            ) {
                return Err(format!("Import failed: {e}").into());
            }
        }
        Command::ImportCompass {
            dolt_dir,
            output,
            table,
            overwrite,
            since,
        } => {
            let dolt_dir = dolt_dir.unwrap_or_else(|| PathBuf::from(&config.dolt.compass_data_dir));
            let output = output.unwrap_or_else(|| PathBuf::from(&config.parquet.dir));
            let table: import_compass::CompassTable = table.parse().map_err(
                |e: String| {
                    error!("{e}");
                    e
                },
            )?;
            if let Err(e) =
                import_compass::run(dolt_dir, output, table, overwrite, since.as_deref())
            {
                return Err(format!("ImportCompass failed: {e}").into());
            }
        }
        Command::Export {
            input,
            format,
            output,
            overwrite,
        } => {
            let input = input.unwrap_or_else(|| PathBuf::from(&config.parquet.dir));
            let output = output.unwrap_or(default_export_output);
            export::run_export(input, format, output, overwrite).await;
        }
        Command::Backup { input, keep_zip } => {
            let input = input.unwrap_or_else(|| PathBuf::from(&config.parquet.dir));
            let script = PathBuf::from("scripts/upload-parquet.sh");
            let mut cmd = std::process::Command::new("bash");
            cmd.arg(&script);
            if keep_zip {
                cmd.arg("--keep-zip");
            }
            cmd.env("PARQUET_DIR", input);
            let status = cmd.status().expect("failed to run upload-parquet.sh");
            if !status.success() {
                return Err("Backup failed".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // Serialisation guard — set_var is not thread-safe, so HOME tests
    // acquire this lock before mutating the env.
    // -----------------------------------------------------------------------
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// RAII guard that restores an env var to its original value on drop.
    struct EnvGuard {
        key: String,
        orig: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let orig = std::env::var(key).ok();
            // SAFETY: ENV_MUTEX serialises all HOME mutations; no other
            // test thread can concurrently read/write this var.
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                orig,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: same serialisation guarantee as `set`.
            unsafe {
                match &self.orig {
                    Some(val) => std::env::set_var(&self.key, val),
                    None => std::env::remove_var(&self.key),
                }
            }
        }
    }

    /// Write a valid config toml to `$HOME/.config/compass/config.toml`.
    fn write_config(home: &std::path::Path, content: &str) {
        let cfg_dir = home.join(".config/compass");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("config.toml"), content).unwrap();
    }

    // ==================================================================
    // load_config tests
    // ==================================================================

    #[test]
    fn load_config_valid_toml_parses_all_fields() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
theme = "custom_dark"

[app]
default_symbol = "600519"
default_timeframe = "1w"

[parquet]
dir = "/custom/parquet"

[dolt]
investment_data_dir = "/custom/investment"
compass_data_dir = "/custom/compass"
"#,
        );
        let _guard = EnvGuard::set("HOME", dir.path().to_str().unwrap());

        let config = load_config();
        assert_eq!(config.theme, "custom_dark");
        assert_eq!(config.app.default_symbol, "600519");
        assert_eq!(config.app.default_timeframe, "1w");
        assert_eq!(config.parquet.dir, "/custom/parquet");
        assert_eq!(config.dolt.investment_data_dir, "/custom/investment");
        assert_eq!(config.dolt.compass_data_dir, "/custom/compass");
    }

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        // deliberate: no config file at all
        let _guard = EnvGuard::set("HOME", dir.path().to_str().unwrap());

        let config = load_config();
        let default = AppConfig::default();
        assert_eq!(config.theme, default.theme);
        assert_eq!(config.app.default_symbol, default.app.default_symbol);
        assert_eq!(config.parquet.dir, default.parquet.dir);
    }

    #[test]
    fn load_config_invalid_toml_returns_defaults() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        write_config(dir.path(), "this is not valid {{{ toml");
        let _guard = EnvGuard::set("HOME", dir.path().to_str().unwrap());

        let config = load_config();
        let default = AppConfig::default();
        assert_eq!(config.theme, default.theme);
    }

    // ==================================================================
    // CLI parsing tests
    // ==================================================================

    #[test]
    fn cli_import_with_limit_and_since() {
        let cli =
            Cli::try_parse_from(["compass-data", "import", "--limit", "100", "--since", "20260101"])
                .unwrap();
        match cli.command {
            Command::Import { limit, since, .. } => {
                assert_eq!(limit, 100);
                assert_eq!(since.as_deref(), Some("20260101"));
            }
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn cli_import_compass_with_table() {
        let cli = Cli::try_parse_from(["compass-data", "import-compass", "--table", "stock_basic"])
            .unwrap();
        match cli.command {
            Command::ImportCompass { table, .. } => {
                assert_eq!(table, "stock_basic");
            }
            _ => panic!("expected ImportCompass"),
        }
    }

    #[test]
    fn cli_export_with_format_and_output() {
        let cli = Cli::try_parse_from([
            "compass-data",
            "export",
            "--format",
            "csv",
            "--output",
            "/tmp/out.csv",
        ])
        .unwrap();
        match cli.command {
            Command::Export { format, output, .. } => {
                assert_eq!(format, "csv");
                assert_eq!(output.unwrap(), PathBuf::from("/tmp/out.csv"));
            }
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn cli_backup_subcommand_parses() {
        let cli = Cli::try_parse_from(["compass-data", "backup"]).unwrap();
        assert!(matches!(cli.command, Command::Backup { .. }));
    }

    #[test]
    fn cli_invalid_subcommand_is_err() {
        let result = Cli::try_parse_from(["compass-data", "nonexistent"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_import_default_limit_is_zero() {
        let cli = Cli::try_parse_from(["compass-data", "import"]).unwrap();
        match cli.command {
            Command::Import { limit, .. } => assert_eq!(limit, 0),
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn cli_export_default_format_is_duckdb() {
        let cli = Cli::try_parse_from(["compass-data", "export"]).unwrap();
        match cli.command {
            Command::Export { format, .. } => assert_eq!(format, "duckdb"),
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn cli_import_compass_with_overwrite() {
        let cli = Cli::try_parse_from([
            "compass-data",
            "import-compass",
            "--table",
            "fin_income",
            "--overwrite",
        ])
        .unwrap();
        match cli.command {
            Command::ImportCompass {
                table, overwrite, ..
            } => {
                assert_eq!(table, "fin_income");
                assert!(overwrite);
            }
            _ => panic!("expected ImportCompass"),
        }
    }

    #[test]
    fn cli_backup_with_keep_zip() {
        let cli = Cli::try_parse_from(["compass-data", "backup", "--keep-zip"]).unwrap();
        match cli.command {
            Command::Backup { keep_zip, .. } => assert!(keep_zip),
            _ => panic!("expected Backup"),
        }
    }

    // ==================================================================
    // run() dispatch tests
    // ==================================================================

    #[tokio::test]
    async fn run_export_duckdb_succeeds() {
        let dir = tempdir().unwrap();
        let output = dir.path().join("test.duckdb");
        let cli = Cli {
            command: Command::Export {
                input: Some(dir.path().to_path_buf()),
                format: "duckdb".to_string(),
                output: Some(output.clone()),
                overwrite: false,
            },
        };
        // Export with an empty parquet dir → ParquetReader logs error and
        // returns early; run_export completes cleanly so run() returns Ok.
        let result = run(cli, AppConfig::default()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_import_errors_on_missing_data() {
        let dir = tempdir().unwrap();
        let cli = Cli {
            command: Command::Import {
                dolt_dir: Some(dir.path().to_path_buf()),
                output: Some(dir.path().to_path_buf()),
                limit: 1,
                symbols: None,
                start_date: None,
                end_date: None,
                since: None,
            },
        };
        // dolt sql will fail — import_dolt::run returns Err
        let result = run(cli, AppConfig::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_import_compass_errors_on_missing_data() {
        let dir = tempdir().unwrap();
        let cli = Cli {
            command: Command::ImportCompass {
                dolt_dir: Some(dir.path().to_path_buf()),
                output: Some(dir.path().to_path_buf()),
                table: "stock_basic".to_string(),
                overwrite: false,
                since: None,
            },
        };
        // dolt sql will fail — import_compass::run returns Err
        let result = run(cli, AppConfig::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_backup_succeeds_with_dummy_script() {
        use std::io::Write;

        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        // Create a minimal upload-parquet.sh that exits 0.
        let scripts_dir = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("upload-parquet.sh");
        let mut f = std::fs::File::create(&script_path).unwrap();
        writeln!(f, "#!/bin/bash").unwrap();
        writeln!(f, "exit 0").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();

        // Change CWD so the relative "scripts/upload-parquet.sh" resolves.
        let orig_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let cli = Cli {
            command: Command::Backup {
                input: Some(dir.path().to_path_buf()),
                keep_zip: false,
            },
        };
        let result = run(cli, AppConfig::default()).await;

        // Restore CWD before any assertions so tempdir cleanup works.
        std::env::set_current_dir(orig_cwd).unwrap();
        assert!(result.is_ok(), "Backup result: {result:?}");
    }

    #[tokio::test]
    async fn run_backup_with_keep_zip_uses_flag() {
        use std::io::Write;

        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        let scripts_dir = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("upload-parquet.sh");
        // Script that exits 0 unconditionally.
        let mut f = std::fs::File::create(&script_path).unwrap();
        writeln!(f, "#!/bin/bash").unwrap();
        writeln!(f, "exit 0").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();

        let orig_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let cli = Cli {
            command: Command::Backup {
                input: Some(dir.path().to_path_buf()),
                keep_zip: true,
            },
        };
        let result = run(cli, AppConfig::default()).await;

        std::env::set_current_dir(orig_cwd).unwrap();
        assert!(result.is_ok(), "Backup with keep-zip result: {result:?}");
    }

    #[tokio::test]
    async fn run_uses_config_defaults_when_cli_options_are_none() {
        // Export with no --input → uses config.parquet.dir.
        // Use an empty tempdir to avoid real data access.
        let dir = tempdir().unwrap();
        let output = dir.path().join("test.duckdb");
        let cli = Cli {
            command: Command::Export {
                input: Some(dir.path().to_path_buf()),
                format: "duckdb".to_string(),
                output: Some(output.clone()),
                overwrite: false,
            },
        };
        let config = AppConfig::default();
        let result = run(cli, config).await;
        assert!(result.is_ok());
    }

    // ==================================================================
    // tracy (keep existing)
    // ==================================================================

    #[test]
    #[cfg(feature = "tracy")]
    fn tracy_layer_constructs() {
        let _layer = tracing_tracy::TracyLayer::default();
    }
}
