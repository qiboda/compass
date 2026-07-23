mod baostock;
mod chunk;
mod download;
mod export;
mod import_dolt;
mod merge;
mod progress;
mod retry;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::error;

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
    },

    /// Merge staging DuckDB into Parquet main database
    Merge {
        /// Staging DuckDB path
        #[arg(long, default_value = "data/staging.duckdb")]
        db: PathBuf,

        /// Main Parquet directory
        #[arg(long, default_value = "parquet_data")]
        output: PathBuf,
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
        #[arg(long, default_value = "compass.duckdb")]
        output: PathBuf,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
            )
            .await;
        }
        Command::Import {
            dolt_dir,
            output,
            limit,
            symbols,
        } => {
            if let Err(e) = import_dolt::run(dolt_dir, output, limit, symbols.as_deref()) {
                error!("Import failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Merge { db, output } => {
            merge::run(db, output).await;
        }
        Command::Export {
            input,
            format,
            output,
        } => {
            export::run_export(input, format, output).await;
        }
    }
}
