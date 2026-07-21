mod baostock;
mod chunk;
mod export;
mod progress;
mod retry;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use clap::Parser;
use compass_rs::data::duckdb::{DailyRecord, DuckDbProvider, StockBasic};
use compass_rs::data::eastmoney::EastMoneyProvider;
use compass_rs::data::provider::DataError;
use compass_rs::data::symbol;
use compass_rs::model::SymbolInfo;
use futures::stream::StreamExt;
use tracing::{info, warn};

use crate::progress::DownloadProgress;
use crate::retry::fetch_bars_with_retry;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "compass-downloader", about = "Download A-share OHLCV data into DuckDB")]
struct Cli {
    /// Stock symbols: "all" or comma-separated codes like "000001,600519"
    #[arg(long, default_value = "all")]
    symbols: String,

    /// Path to DuckDB database file
    #[arg(long, default_value = "compass.duckdb")]
    db: PathBuf,

    /// Maximum concurrent downloads
    #[arg(long, default_value_t = 3)]
    concurrency: usize,

    /// Delay between requests in milliseconds
    #[arg(long, default_value_t = 500)]
    delay_ms: u64,

    /// Start date in YYYYMMDD format (inclusive)
    #[arg(long, default_value = "19900101")]
    start_date: String,

    /// End date in YYYYMMDD format (inclusive). Default: yesterday.
    #[arg(long)]
    end_date: Option<String>,

    /// EastMoney API base URL
    #[arg(long, default_value = "https://push2his.eastmoney.com")]
    base_url: String,

    /// Export all tables as Parquet files to this directory after download
    #[arg(long)]
    export_parquet: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a "YYYYMMDD" string to `DateTime<Utc>` at midnight UTC.
fn yyyymmdd_to_utc(date_str: &str) -> DateTime<Utc> {
    let naive = NaiveDate::parse_from_str(date_str, "%Y%m%d")
        .expect("date must be valid YYYYMMDD");
    let naive_dt = naive
        .and_hms_opt(0, 0, 0)
        .expect("time must be valid");
    DateTime::from_naive_utc_and_offset(naive_dt, Utc)
}

/// Compute yesterday as a "YYYYMMDD" string.
fn yesterday_yyyymmdd() -> String {
    let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
    yesterday.format("%Y%m%d").to_string()
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Resolve end_date
    let end_date_str = cli
        .end_date
        .clone()
        .unwrap_or_else(yesterday_yyyymmdd);

    info!(
        "compass-downloader starting — symbols={}, start={}, end={}, db={}, concurrency={}",
        cli.symbols,
        cli.start_date,
        end_date_str,
        cli.db.display(),
        cli.concurrency,
    );

    // Build HTTP client
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client");

    // Build providers
    let eastmoney = EastMoneyProvider::new(
        http_client,
        cli.base_url.clone(),
        "https://push2.eastmoney.com".to_string(),
    );
    let db = Arc::new(
        DuckDbProvider::new(cli.db.to_str().expect("db path must be valid UTF-8"))
            .expect("failed to open DuckDB"),
    );

    // Enumerate symbols
    let symbol_infos: Vec<SymbolInfo> = if cli.symbols.to_lowercase() == "all" {
        info!("Enumerating all A-share symbols from EastMoney…");
        match eastmoney.search_all_symbols(100, "b:DLMK014").await {
            Ok(list) => {
                info!("Found {} symbols", list.len());
                list
            }
            Err(e) => {
                eprintln!("FATAL: failed to enumerate symbols: {e}");
                std::process::exit(1);
            }
        }
    } else {
        cli.symbols
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|code| SymbolInfo {
                name: code.clone(),
                code,
            })
            .collect()
    };

    if symbol_infos.is_empty() {
        eprintln!("No symbols to process");
        std::process::exit(1);
    }

    // Setup progress display
    let progress = Arc::new(DownloadProgress::new(symbol_infos.len() as u64));
    progress.set_spinner_message("Starting downloads…");

    // Bounded-concurrency pipeline
    let sem = Arc::new(tokio::sync::Semaphore::new(cli.concurrency));

    let results: Vec<(String, Result<usize, String>)> =
        futures::stream::iter(symbol_infos.iter())
            .map(|info| {
                let sem = Arc::clone(&sem);
                let db = Arc::clone(&db);
                let eastmoney = &eastmoney;
                let cli = &cli;
                let progress = Arc::clone(&progress);
                let end_date_str = end_date_str.clone();
                async move {
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    process_symbol(db, eastmoney, info, &cli.start_date, &end_date_str, &cli.delay_ms, &progress).await
                }
            })
            .buffer_unordered(cli.concurrency)
            .collect()
            .await;

    progress.finish();

    // Summarise
    let total = results.len();
    let (successes, failures): (Vec<_>, Vec<_>) =
        results.into_iter().partition(|(_, r)| r.is_ok());

    let total_bars: usize = successes.iter().filter_map(|(_, r)| r.as_ref().ok()).sum();
    let failed = failures.len();

    eprintln!();
    eprintln!("==============================");
    eprintln!(
        "Done: {}/{total} symbols, {total_bars} bars. {failed} failures.",
        successes.len()
    );

    for (symbol, err) in &failures {
        eprintln!("  FAIL {symbol}: {}", err.as_ref().unwrap_err());
    }
    eprintln!("==============================");

    // Parquet export (optional, does not block main flow on failure)
    if let Some(ref export_dir) = cli.export_parquet {
        info!("Exporting Parquet files to {}…", export_dir.display());
        if let Err(e) = export::export_all_tables(&db, export_dir).await {
            warn!("Parquet export failed: {e}");
        }
    }

    if failed > 0 {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Per-symbol pipeline
// ---------------------------------------------------------------------------

async fn process_symbol(
    db: Arc<DuckDbProvider>,
    eastmoney: &EastMoneyProvider,
    info: &SymbolInfo,
    start_date_str: &str,
    end_date_str: &str,
    delay_ms: &u64,
    progress: &DownloadProgress,
) -> (String, Result<usize, String>) {
    let code = &info.code;
    let ts_code = symbol::to_ts_code(code);

    progress.set_spinner_message(&format!("Processing {code}…"));

    // 1. Upsert stock_basic (best-effort — don't fail on this)
    match eastmoney.fetch_stock_basic(code).await {
        Ok(stock_basic) => {
            if let Err(e) = db.upsert_stock_basic(&stock_basic).await {
                tracing::warn!(%code, error = %e, "failed to upsert stock_basic");
            }
        }
        Err(e) => {
            // Best-effort: still insert with minimal info
            tracing::warn!(%code, error = %e, "fetch_stock_basic failed, inserting minimal record");
            let minimal = StockBasic {
                ts_code: ts_code.clone(),
                symbol: info.name.clone(),
                name: info.name.clone(),
                area: None,
                industry: None,
                market: None,
                exchange: Some(symbol::to_exchange(code).to_string()),
                list_date: None,
                delist_date: None,
            };
            if let Err(e2) = db.upsert_stock_basic(&minimal).await {
                tracing::warn!(%code, error = %e2, "failed to upsert minimal stock_basic");
            }
        }
    }

    // 2. Gap detection: figure out what date chunks to fetch
    let start_date = NaiveDate::parse_from_str(start_date_str, "%Y%m%d")
        .expect("start_date must be valid YYYYMMDD");
    let end_date = NaiveDate::parse_from_str(end_date_str, "%Y%m%d")
        .expect("end_date must be valid YYYYMMDD");

    let stored = match db.get_stored_range(&ts_code).await {
        Ok(r) => r,
        Err(e) => return (code.clone(), Err(format!("DB error checking stored range: {e}"))),
    };

    // Determine the effective start/end dates for fetching
    let fetch_ranges: Vec<(String, String)> = match stored {
        None => {
            // No data at all — fetch full range
            vec![(start_date_str.to_string(), end_date_str.to_string())]
        }
        Some((stored_min, stored_max)) => {
            // Only fetch gaps: before stored_min and/or after stored_max
            let mut ranges = Vec::new();
            if start_date < stored_min {
                let gap_end = stored_min - chrono::Duration::days(1);
                ranges.push((
                    start_date.format("%Y%m%d").to_string(),
                    gap_end.format("%Y%m%d").to_string(),
                ));
            }
            if stored_max < end_date {
                let gap_start = stored_max + chrono::Duration::days(1);
                ranges.push((
                    gap_start.format("%Y%m%d").to_string(),
                    end_date_str.to_string(),
                ));
            }
            ranges
        }
    };

    if fetch_ranges.is_empty() {
        // Already fully covered
        progress.inc_symbol(code);
        return (code.clone(), Ok(0));
    }

    // 3. Split each range into max-2000-day chunks
    let mut total_bars: usize = 0;
    for (range_start, range_end) in &fetch_ranges {
        let chunks = chunk::split_date_range(range_start, range_end, 2000);
        for (chunk_start, chunk_end) in &chunks {
            let start_dt = yyyymmdd_to_utc(chunk_start);
            let end_dt = yyyymmdd_to_utc(chunk_end);

            // Fetch with retry
            let bars = match fetch_bars_with_retry(
                eastmoney,
                code,
                "101", // daily
                start_dt,
                end_dt,
                3, // max_attempts
            )
            .await
            {
                Ok(b) => b,
                Err(DataError::NoData { .. }) => {
                    // No data for this chunk — skip and continue
                    tracing::info!(%code, chunk_start, chunk_end, "no data in chunk, skipping");
                    continue;
                }
                Err(e) => {
                    return (
                        code.clone(),
                        Err(format!("fetch failed for {code} [{chunk_start}..{chunk_end}]: {e}")),
                    );
                }
            };

            // Convert Bar → DailyRecord
            let records: Vec<DailyRecord> = bars
                .iter()
                .map(|b| {
                    let trade_date = b.time.date_naive();
                    DailyRecord {
                        trade_date,
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        change: 0.0,   // not available from Bar struct
                        pct_chg: 0.0,   // not available from Bar struct
                        vol: b.volume,
                        amount: 0.0,    // not available from Bar struct
                    }
                })
                .collect();

            let count = records.len();
            total_bars += count;

            // Save to DuckDB
            if let Err(e) = db.save_stock_daily(&ts_code, &records).await {
                return (
                    code.clone(),
                    Err(format!("DB save failed for {code} [{chunk_start}..{chunk_end}]: {e}")),
                );
            }

            tracing::debug!(%code, chunk_start, chunk_end, count, "chunk saved");

            // Rate-limit delay
            if *delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
            }
        }
    }

    progress.inc_symbol(code);
    info!(%code, total_bars, "completed");
    (code.clone(), Ok(total_bars))
}
