//! Criterion benchmarks for EastMoneyProvider.
//!
//! Covers:
//! 1. Pure kline-string parsing latency (single)
//! 2. Pure kline-string parsing throughput (1000)
//! 3. httpmock-backed `fetch_bars` round-trip (100 klines)
//! 4. Error paths — no-data response, bad JSON

use chrono::{DateTime, NaiveDate, Utc};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use egui_charts::model::Bar;

use compass_core::data::eastmoney::EastMoneyProvider;
use compass_core::data::provider::DataProvider;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a realistic EastMoney kline CSV string.
///
/// Format: `date,open,close,high,low,volume,amount,amplitude,updown,change,turnover`
fn kline(date: &str, open: f64, close: f64, high: f64, low: f64, volume: f64) -> String {
    format!("{date},{open},{close},{high},{low},{volume},13000000.00,1.50,0.80,0.10,2.30")
}

/// Parse a single EastMoney kline CSV string into a [`Bar`].
///
/// Duplicates the inline parsing logic from [`EastMoneyProvider::fetch_bars`]
/// so pure parsing can be benchmarked without HTTP or JSON overhead.
fn parse_kline_line(line: &str) -> Option<Bar> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 6 {
        return None;
    }
    let open: f64 = parts[1].parse().ok()?;
    let close: f64 = parts[2].parse().ok()?;
    let high: f64 = parts[3].parse().ok()?;
    let low: f64 = parts[4].parse().ok()?;
    let volume: f64 = parts[5].parse().ok()?;
    let naive = NaiveDate::parse_from_str(parts[0], "%Y-%m-%d").ok()?;
    let naive_dt = naive.and_hms_opt(0, 0, 0)?;
    let time = DateTime::from_naive_utc_and_offset(naive_dt, Utc);
    Some(Bar::new(time, open, high, low, close, volume))
}

// ---------------------------------------------------------------------------
// Benchmark 1: Parse latency — single kline string
// ---------------------------------------------------------------------------

fn bench_parse_latency(c: &mut Criterion) {
    let line = kline("2025-07-21", 12.04, 12.01, 12.11, 11.95, 1_079_027.0);

    c.bench_function("parse_single_kline", |b| {
        b.iter(|| parse_kline_line(black_box(&line)))
    });
}

// ---------------------------------------------------------------------------
// Benchmark 2: Parse throughput — 1000 kline strings
// ---------------------------------------------------------------------------

fn bench_parse_throughput(c: &mut Criterion) {
    let klines: Vec<String> = (0..1000)
        .map(|i| {
            let day = (i % 28) + 1;
            kline(
                &format!("2025-07-{day:02}"),
                10.0 + i as f64 * 0.01,
                10.1 + i as f64 * 0.01,
                10.5 + i as f64 * 0.01,
                9.5 + i as f64 * 0.01,
                100_000.0 + i as f64 * 100.0,
            )
        })
        .collect();

    c.bench_function("parse_1000_klines", |b| {
        b.iter(|| {
            for line in &klines {
                black_box(parse_kline_line(black_box(line)));
            }
        })
    });
}

// ---------------------------------------------------------------------------
// Benchmark 3: httpmock round-trip — fetch_bars with 100 klines
// ---------------------------------------------------------------------------

fn bench_httpmock_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Pre-generate 100 realistic kline strings
    let klines_json: Vec<String> = (0..100)
        .map(|i| {
            let day = (i % 28) + 1;
            kline(
                &format!("2025-07-{day:02}"),
                10.0 + i as f64 * 0.1,
                10.1 + i as f64 * 0.1,
                10.5 + i as f64 * 0.1,
                9.5 + i as f64 * 0.1,
                100_000.0 + i as f64 * 1000.0,
            )
        })
        .collect();

    // Set up the mock server once — the mock stays active for all iterations.
    let server = httpmock::MockServer::start();
    let payload = serde_json::json!({
        "data": { "klines": &klines_json }
    });
    let _mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/api/qt/stock/kline/get");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(payload);
    });

    let provider =
        EastMoneyProvider::new(reqwest::Client::new(), server.base_url(), server.base_url());
    let range_start = chrono::Utc::now() - chrono::Duration::days(365);
    let range_end = chrono::Utc::now();

    c.bench_function("fetch_bars_100_klines", |b| {
        b.iter(|| {
            rt.block_on(async {
                provider
                    .fetch_bars("000001", "1d", range_start, range_end)
                    .await
                    .unwrap()
            })
        })
    });
}

// ---------------------------------------------------------------------------
// Benchmark 4: Error paths
// ---------------------------------------------------------------------------

fn bench_error_paths(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let range_start = chrono::Utc::now() - chrono::Duration::days(365);
    let range_end = chrono::Utc::now();

    // --- 4a: No-data response (empty klines array) ---

    {
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"data": {"klines": []}}));
        });
        let provider =
            EastMoneyProvider::new(reqwest::Client::new(), server.base_url(), server.base_url());

        c.bench_function("fetch_bars_no_data", |b| {
            b.iter(|| {
                rt.block_on(async {
                    black_box(
                        provider
                            .fetch_bars("000001", "1d", range_start, range_end)
                            .await,
                    )
                })
            })
        });
    }

    // --- 4b: Bad JSON (non-JSON response body) ---

    {
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "text/html")
                .body("<html>500 Internal Server Error</html>");
        });
        let provider =
            EastMoneyProvider::new(reqwest::Client::new(), server.base_url(), server.base_url());

        c.bench_function("fetch_bars_bad_json", |b| {
            b.iter(|| {
                rt.block_on(async {
                    black_box(
                        provider
                            .fetch_bars("000001", "1d", range_start, range_end)
                            .await,
                    )
                })
            })
        });
    }
}

criterion_group!(
    benches,
    bench_parse_latency,
    bench_parse_throughput,
    bench_httpmock_roundtrip,
    bench_error_paths,
);
criterion_main!(benches);
