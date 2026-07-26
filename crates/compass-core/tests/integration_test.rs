// =============================================================================
// Integration test: end-to-end pipeline with httpmock + DuckDB :memory:
//
// Mocks the EastMoney K-line API and search_all_symbols endpoint, then runs a
// partial pipeline (enumerate → upsert stock_basic → fetch bars → save
// stock_daily) and verifies correct row counts in the DuckDB tables.
// =============================================================================

use chrono::{DateTime, NaiveDate, Utc};
use compass_core::data::duckdb::{DailyRecord, DuckDbProvider, StockBasic};
use compass_core::data::eastmoney::EastMoneyProvider;
use compass_core::data::provider::DataProvider;
use compass_core::data::symbol;
use compass_core::model::SymbolInfo;
use httpmock::MockServer;

/// Build a single K-line CSV string matching EastMoney format:
/// `date,open,close,high,low,volume,amount,amplitude,pct_chg,change,turnover,...`
fn kline(date: &str, open: f64, close: f64, high: f64, low: f64, volume: f64) -> String {
    format!("{date},{open},{close},{high},{low},{volume},13000000.00,1.50,0.80,0.10,2.30")
}

/// Convert `egui_charts::model::Bar` → `DailyRecord`, extracting trade_date from Bar.time.
fn bar_to_daily(b: &egui_charts::model::Bar) -> DailyRecord {
    let trade_date = b.time.date_naive();
    DailyRecord {
        trade_date,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        adjclose: b.close,
        volume: b.volume,
        amount: 0.0,
    }
}

/// Convert "YYYYMMDD" string to `DateTime<Utc>` at midnight.
fn yyyymmdd_to_utc(date_str: &str) -> DateTime<Utc> {
    let naive = NaiveDate::parse_from_str(date_str, "%Y%m%d").expect("valid date");
    let naive_dt = naive.and_hms_opt(0, 0, 0).expect("valid time");
    DateTime::from_naive_utc_and_offset(naive_dt, Utc)
}

#[tokio::test]
async fn e2e_two_symbols_kline_fetch_and_save_to_duckdb() {
    // ---- 1. Setup httpmock server ----
    let server = MockServer::start_async().await;

    // Mock search_all_symbols — returns 2 codes
    let _m_search = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/clist/get")
            .query_param("pn", "1")
            .query_param("pz", "100");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "diff": [
                        {"f12": "000001", "f14": "平安银行"},
                        {"f12": "600519", "f14": "贵州茅台"},
                    ]
                }
            }));
    });

    // Mock K-line for 000001 — 2 bars (secid = 0.000001)
    let _m_kline_000001 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/stock/kline/get")
            .query_param("secid", "0.000001");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "klines": [
                        kline("2025-07-21", 12.04, 12.01, 12.11, 11.95, 1_079_027.0),
                        kline("2025-07-22", 12.10, 12.20, 12.30, 12.05, 980_000.0),
                    ]
                }
            }));
    });

    // Mock K-line for 600519 — 1 bar (secid = 1.600519)
    let _m_kline_600519 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/stock/kline/get")
            .query_param("secid", "1.600519");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "klines": [
                        kline("2025-07-22", 1500.0, 1510.0, 1520.0, 1490.0, 50_000.0),
                    ]
                }
            }));
    });

    // Mock fetch_stock_basic — single mock returns both stocks (same fs filter now)
    let fs_filter = "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048";
    let _m_basic_all = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/clist/get")
            .query_param("fs", fs_filter);
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "diff": [
                        {
                            "f12": "000001",
                            "f14": "平安银行",
                            "f100": "银行",
                            "f124": -1,
                            "f102": "主板"
                        },
                        {
                            "f12": "600519",
                            "f14": "贵州茅台",
                            "f100": "白酒",
                            "f124": 997920000,
                            "f102": "沪主板"
                        }
                    ]
                }
            }));
    });

    // ---- 2. Build providers ----
    let http_client = reqwest::Client::new();
    let eastmoney = EastMoneyProvider::new(http_client, server.base_url(), server.base_url());
    let db = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    // ---- 3. Enumerate symbols via search_all_symbols ----
    let symbol_infos: Vec<SymbolInfo> = eastmoney
        .search_all_symbols(100, "b:DLMK014")
        .await
        .expect("search_all_symbols failed");
    assert_eq!(
        symbol_infos.len(),
        2,
        "expected 2 symbols from search_all_symbols"
    );
    assert_eq!(symbol_infos[0].code, "000001");
    assert_eq!(symbol_infos[0].name, "平安银行");
    assert_eq!(symbol_infos[1].code, "600519");
    assert_eq!(symbol_infos[1].name, "贵州茅台");

    // ---- 4. For each symbol: upsert stock_basic, fetch bars, save daily ----
    let range_start = yyyymmdd_to_utc("20250701");
    let range_end = yyyymmdd_to_utc("20250730");

    for info in &symbol_infos {
        let code = &info.code;

        // 4a. Upsert stock_basic
        match eastmoney.fetch_stock_basic(code).await {
            Ok(stock_basic) => {
                db.upsert_stock_basic(&stock_basic, true)
                    .await
                    .expect("upsert_stock_basic failed");
            }
            Err(e) => {
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
                db.upsert_stock_basic(&minimal, true).await.expect(&format!(
                    "upsert minimal stock_basic for {code} failed: {e}"
                ));
            }
        }

        // 4b. Fetch bars from EastMoney
        let bars = eastmoney
            .fetch_bars(code, "1d", range_start, range_end)
            .await
            .expect(&format!("fetch_bars for {code} failed"));

        assert!(!bars.is_empty(), "expected at least 1 bar for {code}");

        // 4c. Convert bars → DailyRecord and save to DuckDB
        let records: Vec<DailyRecord> = bars.iter().map(|b| bar_to_daily(b)).collect();
        db.save_stock_daily(code, &records, true)
            .await
            .expect(&format!("save_stock_daily for {code} failed"));

        let stored = db
            .get_stored_range(code)
            .await
            .expect(&format!("get_stored_range for {code} failed"));
        assert!(stored.is_some(), "expected stored range for {code}");
        let (min_d, max_d) = stored.unwrap();
        assert!(min_d <= max_d, "min should be <= max for {code}");
    }

    let (count_000001, count_600519, basic_count) = {
        let conn = db.lock_connection().expect("lock");
        let c1: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_daily WHERE symbol = '000001'",
                [],
                |row| row.get(0),
            )
            .expect("query count for 000001");
        let c2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_daily WHERE symbol = '600519'",
                [],
                |row| row.get(0),
            )
            .expect("query count for 600519");
        let c3: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_basic", [], |row| row.get(0))
            .expect("query stock_basic count");
        (c1, c2, c3)
    };

    assert_eq!(
        count_000001, 2,
        "000001 should have exactly 2 bars in stock_daily"
    );
    assert_eq!(
        count_600519, 1,
        "600519 should have exactly 1 bar in stock_daily"
    );
    assert_eq!(basic_count, 2, "stock_basic should have exactly 2 entries");

    let range_000001 = db
        .get_stored_range("000001")
        .await
        .expect("get_stored_range failed");
    assert!(
        range_000001.is_some(),
        "000001 should have data in stock_daily"
    );

    let range_600519 = db
        .get_stored_range("600519")
        .await
        .expect("get_stored_range failed");
    assert!(
        range_600519.is_some(),
        "600519.SH should have data in stock_daily"
    );

    let sz_info = db
        .get_stock_basic("000001")
        .await
        .expect("get_stock_basic failed");
    assert!(sz_info.is_some(), "000001 should exist in stock_basic");
    let sz = sz_info.unwrap();
    assert_eq!(sz.symbol, "000001");
    assert_eq!(sz.name, "平安银行");
    assert_eq!(sz.exchange.as_deref(), Some("SZ"));

    let sh_info = db
        .get_stock_basic("600519")
        .await
        .expect("get_stock_basic failed");
    assert!(sh_info.is_some(), "600519 should exist in stock_basic");
    let sh = sh_info.unwrap();
    assert_eq!(sh.symbol, "600519");
    assert_eq!(sh.name, "贵州茅台");
    assert_eq!(sh.exchange.as_deref(), Some("SH"));
}

#[tokio::test]
async fn e2e_empty_search_all_symbols_handled_gracefully() {
    let server = MockServer::start_async().await;

    // Mock search_all_symbols — empty result
    let _m_search = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/clist/get")
            .query_param("pn", "1")
            .query_param("pz", "100");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({"data": {"diff": []}}));
    });

    let http_client = reqwest::Client::new();
    let eastmoney = EastMoneyProvider::new(http_client, server.base_url(), server.base_url());

    let results = eastmoney
        .search_all_symbols(100, "b:DLMK014")
        .await
        .expect("search_all_symbols failed");
    assert!(results.is_empty(), "expected empty results");
}

// =============================================================================
// Integration tests against real EastMoney API (network required — #[ignore])
// =============================================================================

/// Test search_all_symbols against real EastMoney API.
/// Uses fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23 (all A-shares).
/// Requires network access — run with: cargo test -- --ignored
#[tokio::test]
#[ignore = "requires network access to EastMoney API"]
async fn e2e_search_all_symbols_real_api_returns_stocks() {
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("build reqwest client");
    let eastmoney = EastMoneyProvider::new(
        http_client,
        "https://push2delay.eastmoney.com".into(),
        "https://push2delay.eastmoney.com".into(),
    );

    let results = eastmoney
        .search_all_symbols(100, "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23")
        .await
        .expect("search_all_symbols should succeed");

    assert!(
        results.len() > 4000,
        "expected >4000 A-share stocks, got {}",
        results.len()
    );

    for info in results.iter().take(10) {
        assert!(!info.code.is_empty(), "stock code should not be empty");
        assert!(
            !info.name.is_empty(),
            "stock name should not be empty for code {}",
            info.code
        );
        assert!(
            info.code.chars().all(|c| c.is_ascii_digit()),
            "stock code should be all digits: {}",
            info.code
        );
    }
}

/// Test fetch_stock_basic against real EastMoney API.
/// Requires network access — run with: cargo test -- --ignored
#[tokio::test]
#[ignore = "requires network access to EastMoney API"]
async fn e2e_fetch_stock_basic_real_api_returns_valid_data() {
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("build reqwest client");
    let eastmoney = EastMoneyProvider::new(
        http_client,
        "https://push2delay.eastmoney.com".into(),
        "https://push2delay.eastmoney.com".into(),
    );

    let info = eastmoney
        .fetch_stock_basic("600519")
        .await
        .expect("fetch_stock_basic for 600519 should succeed");

    assert_eq!(info.symbol, "600519");
    assert!(!info.name.is_empty(), "name should not be empty");
    assert_eq!(info.exchange.as_deref(), Some("SH"));

    let info2 = eastmoney
        .fetch_stock_basic("000001")
        .await
        .expect("fetch_stock_basic for 000001 should succeed");

    assert_eq!(info2.symbol, "000001");
    assert_eq!(info2.exchange.as_deref(), Some("SZ"));
}

/// Test K-line fetch against real EastMoney API.
/// Requires network access — run with: cargo test -- --ignored
#[tokio::test]
#[ignore = "requires network access to EastMoney API"]
async fn e2e_fetch_bars_real_api_returns_data() {
    use chrono::{DateTime, NaiveDate, Utc};
    use compass_core::data::provider::DataProvider;

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("build reqwest client");
    let eastmoney = EastMoneyProvider::new(
        http_client,
        "https://push2his.eastmoney.com".into(),
        "https://push2delay.eastmoney.com".into(),
    );

    let start = DateTime::from_naive_utc_and_offset(
        NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Utc,
    );
    let end = DateTime::from_naive_utc_and_offset(
        NaiveDate::from_ymd_opt(2024, 1, 5)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Utc,
    );

    let bars = eastmoney
        .fetch_bars("600519", "1d", start, end)
        .await
        .expect("fetch_bars should succeed");

    assert!(!bars.is_empty(), "should have bars for 600519");
    for i in 1..bars.len() {
        assert!(
            bars[i].time >= bars[i - 1].time,
            "bars should be sorted by time"
        );
    }
    for bar in &bars {
        assert!(bar.open > 0.0, "open should be positive");
        assert!(bar.high > 0.0, "high should be positive");
        assert!(bar.low > 0.0, "low should be positive");
        assert!(bar.close > 0.0, "close should be positive");
        assert!(bar.volume >= 0.0, "volume should be non-negative");
        assert!(bar.high >= bar.low, "high >= low");
    }
}

// ---------------------------------------------------------------------------
// Test: DuckDB schema integrity — all 7 tables exist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duckdb_in_memory_has_required_tables() {
    let db = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    let tables = [
        "stock_daily",
        "stock_adj_factor",
        "stock_basic",
        "stock_limit",
        "no_data_marks",
    ];

    let conn = db.lock_connection().expect("lock");
    for table in &tables {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = ?1",
                duckdb::params![table],
                |row| row.get(0),
            )
            .expect(&format!("query for table {table}"));
        assert!(exists, "table '{table}' should exist in DuckDB schema");
    }
}

// =============================================================================
// ParquetReader integration tests (requires exported Parquet data)
// =============================================================================

use compass_core::data::parquet::ParquetReader;

#[tokio::test]
#[ignore = "requires exported parquet_data/ — run `cargo run --bin compass-data -- import --limit 3`"]
async fn parquet_reader_loads_exported_data() {
    let parquet_dir = std::path::Path::new("parquet_data");
    if !parquet_dir.exists() {
        panic!("parquet_data/ not found. Run: cargo run --bin compass-data -- import --limit 3");
    }

    let reader = ParquetReader::new(parquet_dir).expect("create ParquetReader");

    let symbols = reader.list_symbols().expect("list_symbols");
    assert!(!symbols.is_empty(), "should have exported symbols");

    // Pick first symbol and verify we can read bars
    let first = &symbols[0];
    let start = chrono::DateTime::from_timestamp(0, 0).unwrap();
    let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).unwrap();

    let bars = reader
        .fetch_bars_blocking(&first.code, start, end)
        .expect("fetch_bars_blocking should succeed");
    assert!(!bars.is_empty(), "{} should have bars", first.code);

    for bar in &bars {
        assert!(bar.open > 0.0, "open should be positive");
        assert!(bar.high >= bar.low, "high >= low");
        assert!(bar.close > 0.0, "close should be positive");
    }

    let range = reader
        .get_stored_range(&first.code)
        .expect("get_stored_range");
    assert!(range.is_some(), "should have stored range");
}
