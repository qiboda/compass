use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use compass_core::data::duckdb::{DailyRecord, DuckDbProvider, StockBasic};
use compass_core::data::eastmoney::EastMoneyProvider;
use compass_core::data::provider::DataError;
use compass_core::data::symbol;
use compass_core::model::SymbolInfo;
use futures::stream::StreamExt;
use tracing::{error, info, warn};

use crate::progress::DownloadProgress;
use crate::retry::fetch_bars_with_retry;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    symbols: String,
    db_path: PathBuf,
    concurrency: usize,
    delay_ms: u64,
    start_date: String,
    end_date: Option<String>,
    base_url: String,
    realtime_url: String,
) {
    let end_date_str = end_date.unwrap_or_else(yesterday_yyyymmdd);

    info!(
        "download starting — symbols={}, start={}, end={}, db={}, concurrency={}",
        symbols,
        start_date,
        end_date_str,
        db_path.display(),
        concurrency,
    );

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client");

    let eastmoney = EastMoneyProvider::new(
        http_client,
        base_url.clone(),
        realtime_url.clone(),
    );
    let db = Arc::new(
        DuckDbProvider::new(db_path.to_str().expect("db path must be valid UTF-8"))
            .expect("failed to open DuckDB"),
    );

    // Enumerate symbols
    let symbol_infos: Vec<SymbolInfo> = if symbols.to_lowercase() == "all" {
        info!("Enumerating all A-share symbols from EastMoney…");
        match eastmoney
            .search_all_symbols(100, "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048")
            .await
        {
            Ok(list) => {
                info!("Found {} symbols", list.len());
                list
            }
            Err(e) => {
                error!("FATAL: failed to enumerate symbols: {e}");
                std::process::exit(1);
            }
        }
    } else {
        symbols
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
        error!("No symbols to process");
        std::process::exit(1);
    }

    // Batch fetch all stock_basic info in one pass (avoids O(N²) per-symbol calls)
    info!("Fetching stock basic info for all symbols...");
    let stock_basics = match eastmoney.fetch_all_stock_basics().await {
        Ok(map) => {
            info!("Got basic info for {} stocks", map.len());
            map
        }
        Err(e) => {
            warn!("Failed to batch-fetch stock basics: {e}. Continuing without metadata.");
            std::collections::HashMap::new()
        }
    };

    // Pre-populate stock_basic table from batch
    for (code, basic) in &stock_basics {
        if let Err(e) = db.upsert_stock_basic(basic).await {
            tracing::warn!(%code, error = %e, "failed to upsert stock_basic from batch");
        }
    }

    let progress = Arc::new(DownloadProgress::new(symbol_infos.len() as u64));
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let stock_basics = Arc::new(stock_basics);

    let results: Vec<(String, Result<usize, String>)> = futures::stream::iter(symbol_infos.iter())
        .map(|info| {
            let sem = Arc::clone(&sem);
            let db = Arc::clone(&db);
            let eastmoney = &eastmoney;
            let progress = Arc::clone(&progress);
            let end_date_str = end_date_str.clone();
            let start_date = start_date.clone();
            let stock_basics = Arc::clone(&stock_basics);
            async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                process_symbol(
                    db,
                    eastmoney,
                    info,
                    &start_date,
                    &end_date_str,
                    &delay_ms,
                    &progress,
                    &stock_basics,
                )
                .await
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    progress.finish();

    let total = results.len();
    let (successes, failures): (Vec<_>, Vec<_>) = results.into_iter().partition(|(_, r)| r.is_ok());
    let total_bars: usize = successes.iter().filter_map(|(_, r)| r.as_ref().ok()).sum();
    let failed = failures.len();

    error!("==============================");
    error!(
        "Done: {}/{total} symbols, {total_bars} bars. {failed} failures.",
        successes.len()
    );

    for (symbol, err) in &failures {
        error!("  FAIL {symbol}: {}", err.as_ref().unwrap_err());
    }
    error!("==============================");

    if failed > 0 {
        std::process::exit(1);
    }
}

/// Convert a "YYYYMMDD" string to `DateTime<Utc>` at midnight UTC.
fn yyyymmdd_to_utc(date_str: &str) -> DateTime<Utc> {
    let naive = NaiveDate::parse_from_str(date_str, "%Y%m%d").expect("date must be valid YYYYMMDD");
    let naive_dt = naive.and_hms_opt(0, 0, 0).expect("time must be valid");
    DateTime::from_naive_utc_and_offset(naive_dt, Utc)
}

/// Compute yesterday as a "YYYYMMDD" string.
fn yesterday_yyyymmdd() -> String {
    let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
    yesterday.format("%Y%m%d").to_string()
}

// ---------------------------------------------------------------------------
// Per-symbol pipeline
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn process_symbol(
    db: Arc<DuckDbProvider>,
    eastmoney: &EastMoneyProvider,
    info: &SymbolInfo,
    start_date_str: &str,
    end_date_str: &str,
    delay_ms: &u64,
    progress: &DownloadProgress,
    stock_basics: &std::collections::HashMap<String, StockBasic>,
) -> (String, Result<usize, String>) {
    let code = &info.code;

    progress.set_spinner_message(&format!("Processing {code}…"));

    // 1. Upsert stock_basic (check batch result first, fall back to HTTP)
    if let Some(stock_basic) = stock_basics.get(code) {
        if let Err(e) = db.upsert_stock_basic(stock_basic).await {
            tracing::warn!(%code, error = %e, "failed to upsert stock_basic from batch");
        }
    } else {
        match eastmoney.fetch_stock_basic(code).await {
            Ok(stock_basic) => {
                if let Err(e) = db.upsert_stock_basic(&stock_basic).await {
                    tracing::warn!(%code, error = %e, "failed to upsert stock_basic");
                }
            }
            Err(e) => {
                tracing::warn!(%code, error = %e, "fetch_stock_basic failed, inserting minimal record");
                let minimal = StockBasic {
                    symbol: code.clone(),
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
    }

    // 2. Gap detection: figure out what date chunks to fetch
    let start_date = NaiveDate::parse_from_str(start_date_str, "%Y%m%d")
        .expect("start_date must be valid YYYYMMDD");
    let end_date =
        NaiveDate::parse_from_str(end_date_str, "%Y%m%d").expect("end_date must be valid YYYYMMDD");

    let stored = match db.get_stored_range(code).await {
        Ok(r) => r,
        Err(e) => {
            return (
                code.clone(),
                Err(format!("DB error checking stored range: {e}")),
            );
        }
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
        let chunks = crate::chunk::split_date_range(range_start, range_end, 2000);
        for (chunk_start, chunk_end) in &chunks {
            let start_dt = yyyymmdd_to_utc(chunk_start);
            let end_dt = yyyymmdd_to_utc(chunk_end);

            // Fetch with retry
            let bars = match fetch_bars_with_retry(
                eastmoney, code, "101", // daily
                start_dt, end_dt, 3, // max_attempts
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
                        Err(format!(
                            "fetch failed for {code} [{chunk_start}..{chunk_end}]: {e}"
                        )),
                    );
                }
            };

            // Convert Bar → DailyRecord
            let records: Vec<DailyRecord> = bars
                .iter()
                .map(|b| DailyRecord {
                    trade_date: b.time.date_naive(),
                    open: b.open,
                    high: b.high,
                    low: b.low,
                    close: b.close,
                    adjclose: b.close,
                    volume: b.volume,
                    amount: 0.0,
                })
                .collect();

            let count = records.len();
            total_bars += count;

            // Save to DuckDB
            if let Err(e) = db.save_stock_daily(code, &records).await {
                return (
                    code.clone(),
                    Err(format!(
                        "DB save failed for {code} [{chunk_start}..{chunk_end}]: {e}"
                    )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use compass_core::data::eastmoney::EastMoneyProvider;
    use httpmock::MockServer;

    fn kline_csv(date: &str, open: f64, close: f64, high: f64, low: f64, volume: f64) -> String {
        format!("{date},{open},{close},{high},{low},{volume},13000000.00,1.50,0.80,0.10,2.30")
    }

    #[test]
    fn yyyymmdd_to_utc_parses_correctly() {
        let dt = yyyymmdd_to_utc("20240102");
        let naive = dt.date_naive();
        assert_eq!(naive, NaiveDate::from_ymd_opt(2024, 1, 2).unwrap());
    }

    #[test]
    fn yyyymmdd_to_utc_handles_leap_year() {
        let dt = yyyymmdd_to_utc("20240229");
        assert_eq!(dt.date_naive(), NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
    }

    #[test]
    fn yesterday_yyyymmdd_is_valid_format() {
        let y = yesterday_yyyymmdd();
        assert_eq!(y.len(), 8);
        y.parse::<i32>().expect("should be numeric");
    }

    #[tokio::test]
    async fn process_symbol_downloads_and_saves_to_db() {
        let server = MockServer::start_async().await;
        let client = reqwest::Client::new();
        let eastmoney = EastMoneyProvider::new(client, server.base_url(), server.base_url());
        let db = Arc::new(DuckDbProvider::new_in_memory().expect("db"));

        // Mock fetch_stock_basic — return on first page
        let fs = "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048";
        let _m_basic = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/clist/get")
                .query_param("fs", fs);
            then.status(200).header("content-type", "application/json").json_body(serde_json::json!({
                "data": {"diff": [{"f12": "000001", "f14": "平安银行", "f100": "银行", "f124": -1, "f102": "主板"}]}
            }));
        });

        // Mock K-line API for secid 0.000001
        let _m_kline = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get")
                .query_param("secid", "0.000001");
            then.status(200).header("content-type", "application/json").json_body(serde_json::json!({
                "data": {"klines": [
                    kline_csv("2024-01-02", 10.0, 10.5, 11.0, 9.5, 1000.0),
                    kline_csv("2024-01-03", 10.5, 11.0, 11.5, 10.0, 2000.0),
                ]}
            }));
        });

        let info = SymbolInfo { code: "000001".into(), name: "平安银行".into() };
        let progress = DownloadProgress::new(1);
        let basics = std::collections::HashMap::new();

        let (code, result) = process_symbol(
            db.clone(), &eastmoney, &info,
            "20240101", "20240105", &0, &progress, &basics,
        ).await;

        assert_eq!(code, "000001");
        let bars = result.expect("should succeed");
        assert_eq!(bars, 2);

        let range = db.get_stored_range("000001").await.expect("range");
        assert!(range.is_some());
    }

    #[tokio::test]
    async fn process_symbol_uses_batch_basics_not_http() {
        let server = MockServer::start_async().await;
        let client = reqwest::Client::new();
        let eastmoney = EastMoneyProvider::new(client, server.base_url(), server.base_url());
        let db = Arc::new(DuckDbProvider::new_in_memory().expect("db"));

        // Pre-populate stock_basic via batch map — no HTTP mock needed for basic
        let mut basics = std::collections::HashMap::new();
        basics.insert("000001".to_string(), StockBasic {
            symbol: "000001".into(), name: "平安银行".into(),
            area: None, industry: None, market: None,
            exchange: Some("SZ".into()),
            list_date: None, delist_date: None,
        });

        // Mock K-line API
        let _m_kline = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get")
                .query_param("secid", "0.000001");
            then.status(200).header("content-type", "application/json").json_body(serde_json::json!({
                "data": {"klines": [
                    kline_csv("2024-06-01", 1500.0, 1510.0, 1520.0, 1490.0, 500.0),
                ]}
            }));
        });

        let info = SymbolInfo { code: "000001".into(), name: "平安银行".into() };
        let progress = DownloadProgress::new(1);

        let (code, result) = process_symbol(
            db.clone(), &eastmoney, &info,
            "20240601", "20240602", &0, &progress, &basics,
        ).await;

        assert_eq!(code, "000001");
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn process_symbol_skips_when_already_covered() {
        let server = MockServer::start_async().await;
        let client = reqwest::Client::new();
        let eastmoney = EastMoneyProvider::new(client, server.base_url(), server.base_url());
        let db = Arc::new(DuckDbProvider::new_in_memory().expect("db"));

        // Pre-populate stock_daily covering the entire date range
        let d = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        let record = DailyRecord {
            trade_date: d,
            open: 10.0, high: 11.0, low: 9.0, close: 10.5,
            adjclose: 10.5, volume: 1000.0, amount: 0.0,
        };
        db.save_stock_daily("000001", &[record]).await.expect("save");

        // No API mocks needed — symbol is already covered
        let info = SymbolInfo { code: "000001".into(), name: "平安银行".into() };
        let progress = DownloadProgress::new(1);
        let basics = std::collections::HashMap::new();

        let (code, result) = process_symbol(
            db.clone(), &eastmoney, &info,
            "20240301", "20240301", &0, &progress, &basics,
        ).await;

        assert_eq!(code, "000001");
        assert_eq!(result.unwrap(), 0); // zero new bars
    }

    #[tokio::test]
    async fn run_downloads_symbols_and_saves_to_db() {
        let server = MockServer::start_async().await;
        let db_dir = tempfile::tempdir().expect("tempdir");
        let db_path = db_dir.path().join("test.duckdb");
        let mock_url = server.base_url();

        // Mock fetch_all_stock_basics (uses realtime_url)
        let fs = "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048";
        let _m_basic = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/clist/get")
                .query_param("fs", fs);
            then.status(200).header("content-type", "application/json").json_body(serde_json::json!({
                "data": {"diff": [
                    {"f12": "000001", "f14": "平安银行", "f100": "", "f124": -1, "f102": ""},
                ]}
            }));
        });

        // Mock K-line for 000001
        let _m_kline = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get")
                .query_param("secid", "0.000001");
            then.status(200).header("content-type", "application/json").json_body(serde_json::json!({
                "data": {"klines": [kline_csv("2024-03-01", 10.0, 10.5, 11.0, 9.5, 1000.0)]}
            }));
        });

        super::run(
            "000001".to_string(), db_path.clone(),
            1, 0,
            "20240301".to_string(), Some("20240302".to_string()),
            mock_url.clone(), mock_url,
        ).await;

        let db = DuckDbProvider::new(db_path.to_str().unwrap()).expect("open");
        let range = db.get_stored_range("000001").await.expect("range");
        assert!(range.is_some(), "data should be saved to DB");
    }
}
