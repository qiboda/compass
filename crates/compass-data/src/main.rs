mod backtest;
mod baostock;
mod export;
mod sepa;
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

        /// Max symbols (0 = all). WARNING: filters + overwrites the whole
        /// stock_daily.parquet with the subset — not an incremental update.
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// Stock symbols to import (comma-separated exchange-prefixed codes,
        /// e.g. "SH600519,sz.000001"; bare 6-digit codes are rejected, dot
        /// form is normalized to Dolt-native prefixed form).
        /// WARNING: filters + overwrites the whole stock_daily.parquet with
        /// only these symbols — not an incremental update.
        #[arg(long)]
        symbols: Option<String>,

        /// Start date (YYYYMMDD), inclusive. WARNING: filters + overwrites the
        /// whole stock_daily.parquet with the date range — not incremental.
        #[arg(long)]
        start_date: Option<String>,

        /// End date (YYYYMMDD), inclusive. WARNING: filters + overwrites the
        /// whole stock_daily.parquet with the date range — not incremental.
        #[arg(long)]
        end_date: Option<String>,

        /// Only export rows with tradedate >= since (YYYYMMDD). WARNING: this is
        /// a filter that overwrites the whole stock_daily.parquet with the subset
        /// — NOT an incremental append. For incremental imports use import-compass.
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

    /// SEPA scoring engine (东方SEPA): score + write-back to Dolt
    Sepa {
        #[command(subcommand)]
        cmd: SepaCmd,
    },
}

/// Nested subcommands of `compass-data sepa`.
#[derive(Subcommand)]
enum SepaCmd {
    /// 计算当日评分并输出 TOP N 表格，写回 Dolt
    Score {
        /// 输出条数上限（默认 50）
        #[arg(long, default_value_t = 50)]
        top: usize,
        /// 指定日期（默认最新交易日；YYYY-MM-DD）
        #[arg(long)]
        date: Option<String>,
    },
    /// 计算市场温度计，写回 Dolt
    Temperature,
    /// 历史批量回测：逐日重算评分，模拟 TOP-N 等权 N 日换仓策略
    Backtest {
        /// 回测窗口起始（默认 2025-01-01；YYYY-MM-DD）
        #[arg(long)]
        start: Option<String>,
        /// 回测窗口结束（默认最新交易日；YYYY-MM-DD）
        #[arg(long)]
        end: Option<String>,
        /// 持仓数量 TOP-N（默认 50）
        #[arg(long, default_value_t = 50)]
        top: usize,
        /// 持有交易日数（默认 5）
        #[arg(long, default_value_t = 5)]
        days: usize,
        /// 单边交易成本比例（默认 0.001）
        #[arg(long, default_value_t = 0.001)]
        cost: f64,
        /// 权益曲线 CSV 输出路径（可选）
        #[arg(long)]
        csv: Option<PathBuf>,
    },
}

impl SepaCmd {
    /// Machine-readable subcommand name used in error messages.
    fn name(&self) -> &'static str {
        match self {
            SepaCmd::Score { .. } => "score",
            SepaCmd::Temperature => "temperature",
            SepaCmd::Backtest { .. } => "backtest",
        }
    }
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
            let table: import_compass::CompassTable = table
                .parse()
                .map_err(|e: String| format!("invalid table: {e}"))?;
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
        Command::Sepa { cmd } => {
            let dolt_dir = PathBuf::from(&config.dolt.compass_data_dir);
            let reader = compass_core::data::parquet::ParquetReader::new(&config.parquet.dir)?;
            let cmd_name = cmd.name();
            match cmd {
                SepaCmd::Score { top, date } => {
                    let date = date
                        .map(|s| {
                            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                                .map_err(|e| format!("invalid --date {s:?}: {e}"))
                        })
                        .transpose()?;
                    if let Err(e) = sepa::run_score(top, date, &reader, &dolt_dir) {
                        return Err(format!("Sepa {cmd_name} failed: {e}").into());
                    }
                }
                SepaCmd::Temperature => {
                    if let Err(e) = sepa::run_temperature(&reader, &dolt_dir) {
                        return Err(format!("Sepa {cmd_name} failed: {e}").into());
                    }
                }
                SepaCmd::Backtest {
                    start,
                    end,
                    top,
                    days,
                    cost,
                    csv,
                } => {
                    let parse = |s: String, flag: &str| {
                        chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                            .map_err(|e| format!("invalid {flag} {s:?}: {e}"))
                    };
                    let start = start.map(|s| parse(s, "--start")).transpose()?;
                    let end = end.map(|s| parse(s, "--end")).transpose()?;
                    if let Err(e) = backtest::run_backtest_cli(
                        top,
                        start,
                        end,
                        days,
                        cost,
                        csv.as_deref(),
                        &reader,
                        &dolt_dir,
                    ) {
                        return Err(format!("Sepa {cmd_name} failed: {e}").into());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // Serialisation guard — set_var is not thread-safe, so HOME tests
    // acquire this lock before mutating the env. `pub(crate)` so the sepa
    // module tests (which spawn `dolt`, a HOME reader) can hold it too.
    // -----------------------------------------------------------------------
    pub(crate) static ENV_MUTEX: Mutex<()> = Mutex::new(());

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
        let cli = Cli::try_parse_from([
            "compass-data",
            "import",
            "--limit",
            "100",
            "--since",
            "20260101",
        ])
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

    #[test]
    fn cli_sepa_score_parses_top_and_date() {
        let cli = Cli::try_parse_from([
            "compass-data",
            "sepa",
            "score",
            "--top",
            "30",
            "--date",
            "2026-07-31",
        ])
        .unwrap();
        match cli.command {
            Command::Sepa { cmd } => match cmd {
                SepaCmd::Score { top, date } => {
                    assert_eq!(top, 30);
                    assert_eq!(date.as_deref(), Some("2026-07-31"));
                }
                _ => panic!("expected Score"),
            },
            _ => panic!("expected Sepa"),
        }
    }

    #[test]
    fn cli_sepa_score_default_top_is_50() {
        let cli = Cli::try_parse_from(["compass-data", "sepa", "score"]).unwrap();
        match cli.command {
            Command::Sepa { cmd } => match cmd {
                SepaCmd::Score { top, date } => {
                    assert_eq!(top, 50);
                    assert_eq!(date, None);
                }
                _ => panic!("expected Score"),
            },
            _ => panic!("expected Sepa"),
        }
    }

    #[test]
    fn cli_sepa_temperature_parses() {
        let cli = Cli::try_parse_from(["compass-data", "sepa", "temperature"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sepa {
                cmd: SepaCmd::Temperature
            }
        ));
    }

    #[test]
    fn cli_sepa_backtest_parses_all_options() {
        let cli = Cli::try_parse_from([
            "compass-data",
            "sepa",
            "backtest",
            "--start",
            "2025-01-01",
            "--end",
            "2026-07-31",
            "--top",
            "30",
            "--days",
            "10",
            "--cost",
            "0.002",
            "--csv",
            "/tmp/curve.csv",
        ])
        .unwrap();
        match cli.command {
            Command::Sepa { cmd } => match cmd {
                SepaCmd::Backtest {
                    start,
                    end,
                    top,
                    days,
                    cost,
                    csv,
                } => {
                    assert_eq!(start.as_deref(), Some("2025-01-01"));
                    assert_eq!(end.as_deref(), Some("2026-07-31"));
                    assert_eq!(top, 30);
                    assert_eq!(days, 10);
                    assert!((cost - 0.002).abs() < 1e-9);
                    assert_eq!(csv.as_deref(), Some(std::path::Path::new("/tmp/curve.csv")));
                }
                _ => panic!("expected Backtest"),
            },
            _ => panic!("expected Sepa"),
        }
    }

    #[test]
    fn cli_sepa_backtest_defaults() {
        let cli = Cli::try_parse_from(["compass-data", "sepa", "backtest"]).unwrap();
        match cli.command {
            Command::Sepa { cmd } => match cmd {
                SepaCmd::Backtest {
                    start,
                    end,
                    top,
                    days,
                    cost,
                    csv,
                } => {
                    assert_eq!(start, None);
                    assert_eq!(end, None);
                    assert_eq!(top, 50);
                    assert_eq!(days, 5);
                    assert!((cost - 0.001).abs() < 1e-9);
                    assert_eq!(csv, None);
                }
                _ => panic!("expected Backtest"),
            },
            _ => panic!("expected Sepa"),
        }
    }

    #[test]
    fn cli_sepa_backtest_rejects_invalid_date() {
        let cli =
            Cli::try_parse_from(["compass-data", "sepa", "backtest", "--start", "not-a-date"])
                .unwrap();
        // Parse succeeds at clap level; the date validation happens in run().
        match cli.command {
            Command::Sepa { cmd } => match cmd {
                SepaCmd::Backtest { start, .. } => {
                    assert_eq!(start.as_deref(), Some("not-a-date"));
                }
                _ => panic!("expected Backtest"),
            },
            _ => panic!("expected Sepa"),
        }
    }

    // ==================================================================
    // import 过滤参数帮助文本警示测试（ref #185）
    // ==================================================================

    /// import 的过滤参数（--symbols/--limit/--start-date/--end-date/--since）
    /// 实际行为是「WHERE 过滤 + 原子覆盖整个 stock_daily.parquet」，不是增量。
    /// 帮助文本必须标注覆盖警示，且不得再用误导性的 "Incremental" 字样
    /// （ref #159 事故诱因正是 help 把 --since 写成 Incremental）。
    #[test]
    fn import_filter_flags_help_warns_overwrite() {
        use clap::CommandFactory;

        let mut cmd = Cli::command();
        let import = cmd
            .find_subcommand_mut("import")
            .expect("import subcommand exists");

        // 每个过滤参数的 help 都必须警告覆盖行为
        let filter_flags = ["symbols", "limit", "start_date", "end_date", "since"];
        for arg in import.get_arguments() {
            let Some(long) = arg.get_long() else {
                continue;
            };
            if filter_flags.contains(&long) {
                let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
                assert!(
                    help.contains("overwrite"),
                    "import --{long} help must warn about full-file overwrite, got: {help}"
                );
            }
        }

        // 全命令帮助文本不得再出现 "Incremental" 字样（import-compass 才是增量）
        let full_help = import.render_long_help().to_string();
        assert!(
            !full_help.contains("Incremental"),
            "import help must not describe filtering as incremental: {full_help}"
        );
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
    #[allow(clippy::await_holding_lock)] // ENV_MUTEX serializes global CWD/env across the whole backup run
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
    #[allow(clippy::await_holding_lock)] // ENV_MUTEX serializes global CWD/env across the whole backup run
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
        // Export with no --input → uses config.parquet.dir (the fallback path).
        // Point config at an empty tempdir so ParquetReader succeeds with no data.
        let dir = tempdir().unwrap();
        let mut config = AppConfig::default();
        config.parquet.dir = dir.path().to_string_lossy().to_string();
        let output = dir.path().join("test.duckdb");
        let cli = Cli {
            command: Command::Export {
                input: None,
                format: "duckdb".to_string(),
                output: Some(output.clone()),
                overwrite: false,
            },
        };
        let result = run(cli, config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // ENV_MUTEX serializes HOME mutation vs dolt spawns
    async fn run_sepa_score_errors_on_missing_dolt() {
        // Valid parquet fixture, but the dolt dir is not a repo → the
        // write-back fails and run() surfaces "Sepa score failed".
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        let parquet_dir = tempdir().unwrap();
        build_minimal_sepa_fixture(parquet_dir.path());
        let mut config = AppConfig::default();
        config.parquet.dir = parquet_dir.path().to_string_lossy().to_string();
        config.dolt.compass_data_dir = dir.path().to_string_lossy().to_string();
        let cli = Cli {
            command: Command::Sepa {
                cmd: SepaCmd::Score {
                    top: 50,
                    date: Some("2026-07-31".to_string()),
                },
            },
        };
        let result = run(cli, config).await;
        assert!(result.is_err());
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("Sepa score failed"), "error: {msg}");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // ENV_MUTEX serializes HOME mutation vs dolt spawns
    async fn run_sepa_temperature_errors_on_missing_dolt() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let dir = tempdir().unwrap();
        let parquet_dir = tempdir().unwrap();
        build_minimal_sepa_fixture(parquet_dir.path());
        let mut config = AppConfig::default();
        config.parquet.dir = parquet_dir.path().to_string_lossy().to_string();
        config.dolt.compass_data_dir = dir.path().to_string_lossy().to_string();
        let cli = Cli {
            command: Command::Sepa {
                cmd: SepaCmd::Temperature,
            },
        };
        let result = run(cli, config).await;
        assert!(result.is_err());
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("Sepa temperature failed"), "error: {msg}");
    }

    /// Minimal fixture shared with the dispatch tests: one stock, one day,
    /// no SEPA aux tables (run_sepa degrades gracefully).
    fn build_minimal_sepa_fixture(dir: &std::path::Path) {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO daily VALUES ('000001', '2026-07-31', 14.0, 15.1, 14.9, 15.0, 15.0, 1.0e6, 5.0e8)",
            [],
        )
        .unwrap();
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            dir.join("stock_daily.parquet").display()
        ))
        .unwrap();
        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO basic VALUES ('000001', '平安银行', 'SZ', '2010-01-01', NULL, '主板', '平安银行', 1.0e9, '测试', NULL)",
            [],
        )
        .unwrap();
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            dir.join("stock_basic.parquet").display()
        ))
        .unwrap();
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
