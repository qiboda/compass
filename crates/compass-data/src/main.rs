mod baostock;
mod chunk;
mod download;
mod export;
use compass_data::import_compass;
use compass_data::import_dolt;
mod merge;
mod progress;
mod retry;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// compass-data — A-share stock data pipeline
///
/// Manages OHLCV data from multiple sources (EastMoney, Dolt) into a
/// Parquet-based main database, with a staging DuckDB for incremental updates.
#[derive(Parser)]
#[command(name = "compass-data")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download OHLCV data from EastMoney into staging DuckDB
    Download {
        /// Stock symbols (comma-separated) or "all"
        #[arg(long, default_value = "all")]
        symbols: String,

        /// Staging DuckDB path
        #[arg(long, default_value = "data/staging.duckdb")]
        db: PathBuf,

        /// Max concurrent downloads
        #[arg(long, default_value_t = 2)]
        concurrency: usize,

        /// Delay between requests in milliseconds
        #[arg(long, default_value_t = 1000)]
        delay_ms: u64,

        /// Start date (YYYYMMDD)
        #[arg(long, default_value = "19900101")]
        start_date: String,

        /// End date (YYYYMMDD), default yesterday
        #[arg(long)]
        end_date: Option<String>,

        /// EastMoney API base URL for K-line data
        #[arg(long, default_value = "https://push2his.eastmoney.com")]
        base_url: String,

        /// EastMoney API URL for realtime/symbol listing
        #[arg(long, default_value = "https://push2delay.eastmoney.com")]
        realtime_url: String,

        /// Overwrite existing data instead of skipping duplicates
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },

    /// Import data from Dolt investment_data into Parquet main database
    Import {
        /// Dolt data directory
        #[arg(long, default_value = "investment_data")]
        dolt_dir: PathBuf,

        /// Output Parquet directory
        #[arg(long, default_value = "parquet_data")]
        output: PathBuf,

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

        /// Overwrite existing data instead of skipping duplicates
        #[arg(long, default_value_t = false)]
        overwrite: bool,

        /// Incremental: only import symbols with tradedate >= since (YYYYMMDD)
        #[arg(long)]
        since: Option<String>,
    },

    /// Import data from compass_data Dolt into Parquet
    ImportCompass {
        /// Dolt data directory
        #[arg(long, default_value = "compass_data")]
        dolt_dir: PathBuf,

        /// Output Parquet directory
        #[arg(long, default_value = "parquet_data")]
        output: PathBuf,

        /// Table to import: stock_basic, fin_indicators
        #[arg(long)]
        table: String,

        /// Overwrite existing data
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },

    /// Merge staging DuckDB into Parquet main database
    Merge {
        /// Staging DuckDB path
        #[arg(long, default_value = "data/staging.duckdb")]
        db: PathBuf,

        /// Main Parquet directory
        #[arg(long, default_value = "parquet_data")]
        output: PathBuf,

        /// Overwrite existing data instead of skipping duplicates
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },

    /// Export Parquet main database to other formats
    Export {
        /// Parquet data directory
        #[arg(long, default_value = "parquet_data")]
        input: PathBuf,

        /// Output format: parquet-dir, duckdb, csv
        #[arg(long, default_value = "duckdb")]
        format: String,

        /// Output path
        #[arg(long, default_value = "data/compass.duckdb")]
        output: PathBuf,

        /// Overwrite existing data instead of skipping duplicates
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
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

    let cli = Cli::parse();

    match cli.command {
        Command::Download {
            symbols,
            db,
            concurrency,
            delay_ms,
            start_date,
            end_date,
            base_url,
            realtime_url,
            overwrite,
        } => {
            download::run(
                symbols,
                db,
                concurrency,
                delay_ms,
                start_date,
                end_date,
                base_url,
                realtime_url,
                overwrite,
            )
            .await;
        }
        Command::Import {
            dolt_dir,
            output,
            limit,
            symbols,
            start_date,
            end_date,
            overwrite,
            since,
        } => {
            if let Err(e) = import_dolt::run(
                dolt_dir,
                output,
                limit,
                symbols.as_deref(),
                start_date.as_deref(),
                end_date.as_deref(),
                overwrite,
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
        } => {
            let table: import_compass::CompassTable = table
                .parse()
                .unwrap_or_else(|e| {
                    error!("{e}");
                    std::process::exit(1);
                });
            if let Err(e) = import_compass::run(dolt_dir, output, table, overwrite) {
                error!("ImportCompass failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Merge {
            db,
            output,
            overwrite,
        } => {
            merge::run(db, output, overwrite).await;
        }
        Command::Export {
            input,
            format,
            output,
            overwrite,
        } => {
            export::run_export(input, format, output, overwrite).await;
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
