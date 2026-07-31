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
                error!("Import failed: {e}");
                std::process::exit(1);
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
            let table: import_compass::CompassTable = table.parse().unwrap_or_else(|e| {
                error!("{e}");
                std::process::exit(1);
            });
            if let Err(e) =
                import_compass::run(dolt_dir, output, table, overwrite, since.as_deref())
            {
                error!("ImportCompass failed: {e}");
                std::process::exit(1);
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
                error!("Backup failed");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "tracy")]
    fn tracy_layer_constructs() {
        let _layer = tracing_tracy::TracyLayer::default();
    }
}
