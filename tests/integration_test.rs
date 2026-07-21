// =============================================================================
// Integration test: end-to-end pipeline with httpmock + DuckDB :memory:
//
// Mocks the EastMoney K-line API and search_all_symbols endpoint, then runs a
// partial pipeline (enumerate → upsert stock_basic → fetch bars → save
// stock_daily) and verifies correct row counts in the DuckDB tables.
// =============================================================================

use chrono::{DateTime, NaiveDate, Utc};
use compass_rs::data::duckdb::{DailyRecord, DuckDbProvider, StockBasic};
use compass_rs::data::eastmoney::EastMoneyProvider;
use compass_rs::data::provider::DataProvider;
use compass_rs::data::symbol;
use compass_rs::model::SymbolInfo;
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
        change: 0.0,
        pct_chg: 0.0,
        vol: b.volume,
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

    // Mock fetch_stock_basic for 000001
    let _m_basic_000001 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/clist/get")
            .query_param("fs", "b:DLMK014,m:000001");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "diff": [{
                        "f12": "000001",
                        "f14": "平安银行",
                        "f100": "银行",
                        "f124": -1,
                        "f102": "主板"
                    }]
                }
            }));
    });

    // Mock fetch_stock_basic for 600519
    let _m_basic_600519 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/clist/get")
            .query_param("fs", "b:DLMK014,m:600519");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": {
                    "diff": [{
                        "f12": "600519",
                        "f14": "贵州茅台",
                        "f100": "白酒",
                        "f124": 997920000,
                        "f102": "沪主板"
                    }]
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
        let ts_code = symbol::to_ts_code(code);

        // 4a. Upsert stock_basic
        match eastmoney.fetch_stock_basic(code).await {
            Ok(stock_basic) => {
                db.upsert_stock_basic(&stock_basic)
                    .await
                    .expect("upsert_stock_basic failed");
            }
            Err(e) => {
                // Best-effort fallback with minimal record
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
                db.upsert_stock_basic(&minimal).await.expect(&format!(
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
        db.save_stock_daily(&ts_code, &records)
            .await
            .expect(&format!("save_stock_daily for {code} failed"));

        // Verify stored range is correct
        let stored = db
            .get_stored_range(&ts_code)
            .await
            .expect(&format!("get_stored_range for {code} failed"));
        assert!(stored.is_some(), "expected stored range for {code}");
        let (min_d, max_d) = stored.unwrap();
        assert!(min_d <= max_d, "min should be <= max for {code}");
    }

    // All direct conn queries in ONE scope: async db methods internally lock conn too.
    let (count_000001, count_600519, basic_count) = {
        let conn = db.conn.lock().expect("lock");
        let c1: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_daily WHERE ts_code = '000001.SZ'",
                [],
                |row| row.get(0),
            )
            .expect("query count for 000001.SZ");
        let c2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_daily WHERE ts_code = '600519.SH'",
                [],
                |row| row.get(0),
            )
            .expect("query count for 600519.SH");
        let c3: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_basic", [], |row| row.get(0))
            .expect("query stock_basic count");
        (c1, c2, c3)
    };

    assert_eq!(
        count_000001, 2,
        "000001.SZ should have exactly 2 bars in stock_daily"
    );
    assert_eq!(
        count_600519, 1,
        "600519.SH should have exactly 1 bar in stock_daily"
    );
    assert_eq!(basic_count, 2, "stock_basic should have exactly 2 entries");

    let range_000001 = db
        .get_stored_range("000001.SZ")
        .await
        .expect("get_stored_range failed");
    assert!(
        range_000001.is_some(),
        "000001.SZ should have data in stock_daily"
    );

    let range_600519 = db
        .get_stored_range("600519.SH")
        .await
        .expect("get_stored_range failed");
    assert!(
        range_600519.is_some(),
        "600519.SH should have data in stock_daily"
    );

    let sz_info = db
        .get_stock_basic("000001.SZ")
        .await
        .expect("get_stock_basic failed");
    assert!(sz_info.is_some(), "000001.SZ should exist in stock_basic");
    let sz = sz_info.unwrap();
    assert_eq!(sz.symbol, "000001");
    assert_eq!(sz.name, "平安银行");
    assert_eq!(sz.exchange.as_deref(), Some("SZ"));

    let sh_info = db
        .get_stock_basic("600519.SH")
        .await
        .expect("get_stock_basic failed");
    assert!(sh_info.is_some(), "600519.SH should exist in stock_basic");
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

// ---------------------------------------------------------------------------
// Test: DuckDB schema integrity — all 7 tables exist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duckdb_in_memory_has_all_seven_tables() {
    let db = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    let tables = [
        "stock_daily",
        "stock_adj_factor",
        "stock_basic",
        "stock_status",
        "stock_limit",
        "daily_indicator",
        "stock_share",
        "no_data_marks",
    ];

    let conn = db.conn.lock().expect("lock");
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
