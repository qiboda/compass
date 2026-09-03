use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config::csv_dir;
use crate::csv::write_csv;
use crate::dolt::import_replace_table;
use crate::error::{CollectError, Result};
use crate::http::{HttpClient, SINA_MIN_INTERVAL, Throttle};
use crate::progress::Progress;

/// Sina lscjfb main-capital-flow report name.
pub const REPORT_NAME: &str = "RPT_MAIN_MONEY_FLOW";
/// Dolt target table name.
pub const DOLT_TABLE: &str = "capital_main_flow";
/// Source label recorded in `data_updates` for this table.
pub const SOURCE: &str = "Sina MoneyFlow ssl_qsfx_lscjfb";

/// Sina per-symbol daily main-capital-flow endpoint (`MoneyFlow.ssl_qsfx_lscjfb`).
pub const SINA_URL: &str =
    "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/MoneyFlow.ssl_qsfx_lscjfb";
/// Daily incremental window: rows returned per symbol per request.
pub const SINA_DAILY_NUM: usize = 20;
/// Historical backfill window: rows returned per symbol per request.
pub const SINA_BACKFILL_NUM: usize = 1000;
/// Per-symbol backfill retry count before the whole batch aborts (#342).
pub const SINA_BACKFILL_RETRIES: u32 = 3;
/// Base backoff for backfill retries; per-attempt wait is
/// `SINA_BACKFILL_BACKOFF * 2^attempt` (2s/4s, same formula as the daily
/// window in `fetch_symbol_window`).
const SINA_BACKFILL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);

fn sina_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), crate::http::EM_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    headers.insert(
        "Referer".to_string(),
        "https://finance.sina.com.cn/".to_string(),
    );
    headers
}

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS capital_main_flow (
    symbol              VARCHAR(20) NOT NULL,
    trade_date          DATE NOT NULL,
    main_net_inflow     DOUBLE,
    main_net_inflow_rate DOUBLE,
    super_large_net     DOUBLE,
    large_net           DOUBLE,
    medium_net          DOUBLE,
    small_net           DOUBLE,
    update_date         DATE,
    PRIMARY KEY (symbol, trade_date)
)"#;

const INSERT_COLS: &str = "main_net_inflow, main_net_inflow_rate, super_large_net, large_net, medium_net, small_net, update_date";

#[derive(Serialize, Clone)]
struct FlowRecord {
    symbol: String,
    trade_date: String,
    main_net_inflow: String,
    main_net_inflow_rate: String,
    super_large_net: String,
    large_net: String,
    medium_net: String,
    small_net: String,
    update_date: String,
}

fn today() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn normalize_num(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.is_empty() || s == "-" => String::new(),
        Some(Value::String(s)) => match s.parse::<f64>() {
            Ok(n) => n.to_string(),
            Err(_) => s.clone(),
        },
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

fn sina_num(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() || s == "-" {
                0.0
            } else {
                s.parse::<f64>().unwrap_or(0.0)
            }
        }
        Some(_) => 0.0,
    }
}

fn fmt_value(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    // Round-trip through normalize_num keeps the shortest f64 representation
    // ("5", "0.02") and the blank semantics locked by its tests.
    normalize_num(Some(&Value::String(v.to_string())))
}

/// Sina query key: `SH600519` → `sh600519` (prefix is already SH/SZ/BJ).
fn daima(symbol: &str) -> String {
    symbol.to_lowercase()
}

/// Parse one `MoneyFlow.ssl_qsfx_lscjfb` row into a `FlowRecord`.
/// `opendate` is mandatory (a missing date cannot be an importable row);
/// all amounts are numeric defaults (0.0) and the rate uses the main-force
/// share of total turnover as a *percent* (the EastMoney f184 unit the
/// historical rows use): (r0_net+r1_net)/(r0+r1+r2+r3) × 100, 0 on a zero
/// sum. `ratioamount` is deliberately unused — its denominator is the
/// whole-market net amount, not this symbol's turnover.
fn parse_sina_row(symbol: &str, row: &Value) -> Option<FlowRecord> {
    let trade_date = row.get("opendate")?.as_str()?.trim().to_string();
    if trade_date.is_empty() {
        return None;
    }
    let r0 = sina_num(row.get("r0"));
    let r1 = sina_num(row.get("r1"));
    let r2 = sina_num(row.get("r2"));
    let r3 = sina_num(row.get("r3"));
    let r0_net = sina_num(row.get("r0_net"));
    let r1_net = sina_num(row.get("r1_net"));
    let r2_net = sina_num(row.get("r2_net"));
    let r3_net = sina_num(row.get("r3_net"));
    let main_net_inflow = r0_net + r1_net;
    let denominator = r0 + r1 + r2 + r3;
    let rate = if denominator == 0.0 {
        0.0
    } else {
        (main_net_inflow / denominator) * 100.0
    };
    Some(FlowRecord {
        symbol: symbol.to_string(),
        trade_date,
        main_net_inflow: fmt_value(main_net_inflow),
        main_net_inflow_rate: fmt_value(rate),
        super_large_net: fmt_value(r0_net),
        large_net: fmt_value(r1_net),
        medium_net: fmt_value(r2_net),
        small_net: fmt_value(r3_net),
        update_date: today(),
    })
}

/// Keep only rows strictly newer than the last report date (window increment).
/// `None` anchor (first run) keeps every fetched row.
fn filter_daily_window(rows: Vec<FlowRecord>, last_report_date: Option<&str>) -> Vec<FlowRecord> {
    match last_report_date {
        Some(anchor) => rows
            .into_iter()
            .filter(|r| r.trade_date.as_str() > anchor)
            .collect(),
        None => rows,
    }
}

/// Write a non-empty CSV; or, for zero rows (a no-op window), remove any
/// stale CSV and return `Ok` so the pipeline treats it as a successful no-op.
fn finalize_daily_csv(output: &Path, rows: Vec<FlowRecord>) -> Result<PathBuf> {
    if rows.is_empty() {
        let _ = std::fs::remove_file(output);
    } else {
        write_csv(output, &rows)?;
    }
    Ok(output.to_path_buf())
}

/// `[start, end]` inclusive membership (ISO dates compare lexicographically).
fn in_backfill_range(day: &str, start: &str, end: &str) -> bool {
    day >= start && day <= end
}

/// Extract the in-range records of one Sina backfill page (pure).
///
/// #342: parse + range-filter logic extracted from the `backfill` loop so a
/// per-symbol window can be retried as a unit without re-parsing the page, and
/// tested without any network. Unparseable rows are skipped (same semantics as
/// the inline loop); rows are yielded in page order and deduplication by
/// (symbol, day) remains in the caller's `seen` map (pre-#342 semantics).
fn extract_backfill_window(symbol: &str, data: &Value, start: &str, end: &str) -> Vec<FlowRecord> {
    let Value::Array(items) = data else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|row| parse_sina_row(symbol, row))
        .filter(|record| in_backfill_range(&record.trade_date, start, end))
        .collect()
}

/// Retry a single-symbol Sina backfill fetch up to `attempts` times with
/// exponential backoff (`backoff * 2^attempt`); Ok on first success, Err
/// after exhaustion. #342: same backoff formula as the daily window
/// (`fetch_symbol_window`, 2s/4s); the caller aborts the whole batch on Err.
///
/// `attempts` is the total number of op invocations (>= 1); 0 is rejected
/// with `InvalidInput` — an empty loop would otherwise fall through to the
/// trailing `unreachable!` and lie about the retry count. The delay shifts
/// run only for `attempt < attempts - 1`, so `attempts <= 33` keeps every
/// `1u32 << attempt` in range (the sole caller passes 3).
async fn retry_sina_backfill<F, Fut>(
    symbol: &str,
    attempts: u32,
    backoff: std::time::Duration,
    mut op: F,
) -> Result<Vec<FlowRecord>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<FlowRecord>>>,
{
    if attempts == 0 {
        return Err(CollectError::InvalidInput(
            "retry_sina_backfill: attempts must be >= 1".to_string(),
        ));
    }
    debug_assert!(
        attempts <= 33,
        "2^attempt must stay in u32 range (max shift index is attempts - 2)"
    );
    for attempt in 0..attempts {
        match op().await {
            Ok(rows) => return Ok(rows),
            Err(e) => {
                if attempt + 1 < attempts {
                    // Plan #342: 2s/4s exponential backoff, same formula as the
                    // daily window (`backoff * 2^attempt`). Invariant: the
                    // sleeps must stay >= SINA_MIN_INTERVAL (100ms) because the
                    // throttle is only re-armed on the next symbol's acquire,
                    // not between retries of one symbol — see backfill().
                    let wait = backoff * (1u32 << attempt);
                    eprintln!(
                        "    retry {}/{} for {symbol} in {wait:?}: {e}",
                        attempt + 1,
                        attempts
                    );
                    tokio::time::sleep(wait).await;
                } else {
                    return Err(CollectError::BackfillSymbolFailed {
                        symbol: symbol.to_string(),
                        attempts,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }
    unreachable!("retry loop always returns")
}

/// Fetch the newest per-symbol window from Sina (`num=20`, page 1). A `null`
/// body (e.g. uncovered BJ tickers) parses as an empty window, never an error.
async fn fetch_symbol_window(
    client: &HttpClient,
    throttle: &mut Throttle,
    symbol: &str,
) -> Result<Vec<FlowRecord>> {
    let mut params = HashMap::new();
    params.insert("page".to_string(), "1".to_string());
    params.insert("num".to_string(), SINA_DAILY_NUM.to_string());
    params.insert("sort".to_string(), "opendate".to_string());
    params.insert("asc".to_string(), "0".to_string());
    params.insert("daima".to_string(), daima(symbol));

    for attempt in 0..3 {
        throttle.acquire().await;
        match client
            .get_json_with_headers_and_proxy(SINA_URL, &params, &sina_headers(), None)
            .await
        {
            Ok(Value::Array(items)) => {
                return Ok(items
                    .iter()
                    .filter_map(|row| parse_sina_row(symbol, row))
                    .collect());
            }
            Ok(_) => return Ok(Vec::new()),
            Err(e) => {
                if attempt < 2 {
                    // Plan #339: 2s/4s exponential backoff between retries.
                    let wait = std::time::Duration::from_secs(2u64 << attempt);
                    eprintln!("    retry {}/3 for {symbol} in {wait:?}: {e}", attempt + 1);
                    tokio::time::sleep(wait).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
    unreachable!("retry loop always returns")
}

/// Fetch the latest main capital flow window per symbol into a CSV.
pub async fn run() -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}.csv"));
    let today = today();

    let last = crate::dolt::last_report_date(DOLT_TABLE).await?;
    if last.as_deref() == Some(today.as_str()) {
        eprintln!("Data up to date ({today}); skipping fetch");
        return Ok(output_path);
    }

    let symbols = active_symbols(&today, &today).await?;
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(SINA_MIN_INTERVAL);
    let mut progress = Progress::new("main_flow", None, Some(output_path.clone()), "start")?;

    let mut all_rows = Vec::new();
    let mut failed = 0u32;
    for symbol in &symbols {
        match fetch_symbol_window(&client, &mut throttle, symbol).await {
            Ok(rows) => all_rows.extend(rows),
            Err(e) => {
                failed += 1;
                eprintln!("[main_flow] {symbol}: window fetch failed, skipping: {e}");
            }
        }
    }
    if failed > 0 {
        eprintln!(
            "[main_flow] WARNING: {failed} of {} symbols failed this window — \
             affected symbol rows may be missing until the next run",
            symbols.len()
        );
    }
    let _ = progress.update(
        Some(symbols.len() as u64),
        Some(all_rows.len() as u64),
        None,
        Some(format!(
            "Fetched {} rows from {} symbols, {failed} failed",
            all_rows.len(),
            symbols.len()
        )),
        None,
    );

    let records = filter_daily_window(all_rows, last.as_deref());
    if records.is_empty() {
        eprintln!("[main_flow] No rows newer than {last:?} — treating as no-op ({today})");
        let _ = progress.finish(Some(0), "no new rows");
        return finalize_daily_csv(&output_path, records);
    }
    eprintln!("[main_flow] {today} window: {} new rows", records.len());
    let _ = progress.finish(Some(records.len() as u64), "Done");
    finalize_daily_csv(&output_path, records)
}

/// Import the fetched CSV into Dolt `capital_main_flow` (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} (symbol, trade_date, {INSERT_COLS}) \
         SELECT symbol, trade_date, {INSERT_COLS} FROM _tmp_mf \
         WHERE symbol IN (SELECT symbol FROM stock_basic) \
           AND main_net_inflow IS NOT NULL",
    );
    import_replace_table(
        &path,
        "_tmp_mf",
        DDL,
        &insert_sql,
        DOLT_TABLE,
        SOURCE,
        "MAX(trade_date)",
        None,
        true,
    )
    .await
}

// ── Historical per-symbol backfill (issue #308, Sina lscjfb) ─────────────

/// Active-symbol SQL: rows whose listing/cancellation dates overlap the
/// requested window — never fetch delisted or not-yet-listed stocks. An
/// empty `list_date` string is kept (Dolt `CAST('' AS DATE)` is NULL, so
/// the plain `IS NULL OR CAST(...)` form would drop such rows; batch-1
/// boundary test SH600002 locks this). `start`/`end` are pre-validated ISO
/// dates by the callers (`backfill` parses them before use, `run` uses
/// `today()`), so no quoting defence is needed here.
fn active_symbols_sql(start: &str, end: &str) -> String {
    format!(
        "SELECT symbol FROM stock_basic \
         WHERE (list_date IS NULL OR list_date = '' OR CAST(list_date AS DATE) <= '{end}') \
           AND (delist_date IS NULL OR delist_date >= '{start}') \
         ORDER BY symbol"
    )
}

/// Parse `dolt sql -r csv` output of `active_symbols_sql` into a symbol
/// list: skip the header line, blank lines and the literal `NULL` text
/// (Dolt CSV's missing-value shape — a bare "filter empty" would leak the
/// `NULL` text line in as a fetch target).
fn parse_symbol_csv(out: &str) -> Vec<String> {
    out.lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "NULL")
        .map(ToString::to_string)
        .collect()
}

/// Project `symbols` onto `active`, preserving input order and duplicates
/// (exact match, no normalisation — the pre-#348 backfill never normalised
/// symbols; `daima()` lowercases only the Sina query key).
fn filter_active_symbols(symbols: &[String], active: &HashSet<String>) -> Vec<String> {
    symbols
        .iter()
        .filter(|s| active.contains(*s))
        .cloned()
        .collect()
}

/// Active symbols for `[start, end]`: every stock listed on or before `end`
/// and not delisted before `start` (at least one listed day in range).
/// Without a Dolt repo (test/dev) the pre-#348 fallback `["SH600519"]` is
/// kept so unit tests stay network-hermetic.
async fn active_symbols(start: &str, end: &str) -> Result<Vec<String>> {
    let dir = crate::config::dolt_dir();
    if dir.join(".dolt").exists() {
        let out = crate::dolt::dolt_sql_csv(&active_symbols_sql(start, end)).await?;
        let symbols = parse_symbol_csv(&out);
        if symbols.is_empty() {
            return Err(CollectError::InvalidInput(
                "active symbols: stock_basic contains no symbols".into(),
            ));
        }
        return Ok(symbols);
    }
    Ok(vec!["SH600519".to_string()])
}

/// Fetch missing per-symbol historical main capital flow via Sina lscjfb.
pub async fn backfill(start: &str, end: &str, symbols: Option<&[String]>) -> Result<PathBuf> {
    let start_dt = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: "start".into(),
            value: start.into(),
        }
    })?;
    let end_dt = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: "end".into(),
            value: end.into(),
        }
    })?;
    if start_dt > end_dt {
        return Err(CollectError::InvertedRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }

    let active = active_symbols(start, end).await?;
    let symbol_list = match symbols {
        Some(s) if s.is_empty() => {
            // Empty request list is a caller mistake, not a window filter
            // result: keep the original wording (QA nit for #348).
            return Err(CollectError::InvalidInput(
                "backfill: no symbols to fetch".to_string(),
            ));
        }
        Some(s) => {
            let active_set: HashSet<String> = active.iter().cloned().collect();
            let filtered = filter_active_symbols(s, &active_set);
            if filtered.is_empty() {
                return Err(CollectError::InvalidInput(format!(
                    "backfill: all {} requested symbols are outside the active window \
                     in {start}..{end}",
                    s.len()
                )));
            }
            filtered
        }
        None => active,
    };

    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}_backfill.csv"));
    let mut seen: HashMap<(String, String), FlowRecord> = HashMap::new();
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(SINA_MIN_INTERVAL);

    for symbol in &symbol_list {
        // One throttle acquire per symbol (pre-#342 pacing); retry attempts are
        // separated by the exponential backoff inside the runner anyway. The
        // retry closure captures only shared references — `F: FnMut() -> Fut`
        // rejects `&mut` captures (the future would escape the closure body).
        throttle.acquire().await;
        let rows = retry_sina_backfill(
            symbol,
            SINA_BACKFILL_RETRIES,
            SINA_BACKFILL_BACKOFF,
            || async {
                let mut params = HashMap::new();
                params.insert("page".to_string(), "1".to_string());
                params.insert("num".to_string(), SINA_BACKFILL_NUM.to_string());
                params.insert("sort".to_string(), "opendate".to_string());
                params.insert("asc".to_string(), "0".to_string());
                params.insert("daima".to_string(), daima(symbol));

                let data = client
                    .get_json_with_headers_and_proxy(SINA_URL, &params, &sina_headers(), None)
                    .await?;
                Ok(extract_backfill_window(symbol, &data, start, end))
            },
        )
        .await?;

        for record in rows {
            let day = record.trade_date.clone();
            seen.insert((symbol.clone(), day), record);
        }
    }

    if seen.is_empty() {
        return Err(CollectError::InvalidInput(format!(
            "backfill: no sina data returned for {} symbols in {start}..{end}",
            symbol_list.len()
        )));
    }

    let mut records: Vec<FlowRecord> = seen.into_values().collect();
    records.sort_by(|a, b| {
        a.trade_date
            .cmp(&b.trade_date)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    write_csv(&output_path, &records)?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_num_blank_and_dash() {
        assert_eq!(normalize_num(Some(&Value::String("-".into()))), "");
        assert_eq!(normalize_num(None), "");
        assert_eq!(normalize_num(Some(&serde_json::json!(1.2))), "1.2");
    }

    #[tokio::test]
    async fn backfill_rejects_inverted_before_network() {
        let err = backfill("2026-08-28", "2026-08-27", Some(&[]))
            .await
            .unwrap_err();
        assert!(matches!(err, CollectError::InvertedRange { .. }));
    }

    // ── Adversarial: Sina migration contract (#339) ──────────────────────
    //
    // RED rationale: the production code before #339 still carries the
    // EastMoney push2 code path.  The tests below reference only plan-declared
    // symbols (SOURCE value, SINA_* constants, run() signature) plus the
    // remaining stable helpers, so they RED as either assertion failures or
    // compile-time "interface not landed yet" failures, and GREEN once the
    // documented implementation lands.

    /// data_updates SOURCE label must be switched to the Sina provider
    /// (plan #339: SOURCE = "Sina MoneyFlow ssl_qsfx_lscjfb").
    /// RED before #339: the constant still says "EastMoney push2 clist f62".
    #[test]
    fn source_label_is_sina_lscjfb() {
        assert_eq!(
            SOURCE, "Sina MoneyFlow ssl_qsfx_lscjfb",
            "data_updates SOURCE must record the Sina lscjfb endpoint"
        );
    }

    /// Plan-declared request geometry: daily window uses num=20, history
    /// backfill uses num=1000, both against the exact lscjfb URL.
    /// RED before #339: SINA_URL / SINA_DAILY_NUM / SINA_BACKFILL_NUM do not
    /// exist yet (compile error) — the constants are part of the plan contract.
    #[test]
    fn sina_request_constants_match_plan() {
        assert_eq!(
            SINA_URL,
            "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/MoneyFlow.ssl_qsfx_lscjfb"
        );
        assert_eq!(SINA_DAILY_NUM, 20, "daily incremental window per plan #339");
        assert_eq!(SINA_BACKFILL_NUM, 1000, "backfill page size per plan #339");
    }

    /// Compile-time signature guard: `run()` must drop its `page_size`
    /// parameter (plan #339) and keep returning `Result<PathBuf>`.
    /// The future is constructed but NEVER polled, so the async body (network /
    /// Dolt / CSV) cannot execute — this is a pure signature check.
    #[test]
    fn run_signature_drops_page_size() {
        let _typed: std::pin::Pin<Box<dyn std::future::Future<Output = Result<PathBuf>>>> =
            Box::pin(run());
    }

    /// normalize_num must treat a blank string exactly like NULL/- (all three
    /// are "missing number" per the Sina contract), and must never panic on
    /// non-numeric JSON types. Adversarial: a stub that only handles "-" but
    /// returns "-" or "" for the empty string would silently poison the CSV.
    #[test]
    fn normalize_num_blank_string_and_weird_types_do_not_panic() {
        assert_eq!(normalize_num(Some(&Value::String(String::new()))), "");
        assert_eq!(normalize_num(Some(&Value::Bool(true))), "true");
        // A JSON array element (structure) must not panic; any deterministic
        // string output is acceptable, but a panic is not.
        let _ = normalize_num(Some(&serde_json::json!([1, 2, 3])));
        let _ = normalize_num(Some(&serde_json::json!({"a": 1})));
    }

    /// backfill with an explicit empty symbol list must fail BEFORE any
    /// network I/O (plan: "seen.is_empty() → Err" carries over; a silent
    /// Ok(()) here would let callers believe history was backfilled).
    /// Adversarial: guards the backfill path against a no-op regression while
    /// the run() path gets its own weekend no-op semantics in #338.
    #[tokio::test]
    async fn backfill_empty_symbols_errors_before_network() {
        let err = backfill("2026-08-28", "2026-08-28", Some(&[]))
            .await
            .unwrap_err();
        assert!(
            matches!(err, CollectError::InvalidInput(_)),
            "empty symbol list must be rejected, got {err:?}"
        );
    }

    /// Date boundary attack: month 13 is not a valid month component; the
    /// backfill entry point must reject it with InvalidDate before touching
    /// the network or the (empty) symbol list.
    #[tokio::test]
    async fn backfill_malformed_start_date_rejected_before_network() {
        let err = backfill("2026-13-45", "2026-08-28", Some(&[]))
            .await
            .unwrap_err();
        assert!(matches!(err, CollectError::InvalidDate { .. }));
    }

    // ── Requirement: Sina daily window contract (#339) ────────────────────
    //
    // Acceptance contract from plan fix-mainflow-sina-remove-sepa (#339):
    // daima() lowercase mapping, sina row field mapping/rate/division-by-zero,
    // num=20 window filtering (trade_date > anchor; anchor == None keeps all),
    // zero-new-row semantics (stale CSV removed, Ok returned), backfill
    // [start,end] inclusive filtering.  All functions are NEW — RED is a
    // compile-time "interface not landed yet" failure before #339.

    #[test]
    fn daima_lowercases_when_building_sina_query_key() {
        assert_eq!(daima("SH600519"), "sh600519");
        assert_eq!(daima("SZ000001"), "sz000001");
        assert_eq!(daima("BJ830001"), "bj830001");
        assert_eq!(daima("BJ920000"), "bj920000");
    }

    #[test]
    fn parse_sina_row_maps_fields_and_calculates() {
        let row = serde_json::json!({
            "opendate": "2026-08-28",
            "trade": "sh600519",
            "num": 12345,
            "r0": "100", "r0_ratio": "1", "r0_net": "2",
            "r1": "50", "r1_ratio": "1", "r1_net": "3",
            "r2": "50", "r2_ratio": "1", "r2_net": "4",
            "r3": "50", "r3_ratio": "1", "r3_net": "5",
            "netamount": "14", "ratioamount": "0.05"
        });
        let r = parse_sina_row("SH600519", &row).expect("valid sina row");
        assert_eq!(r.symbol, "SH600519");
        assert_eq!(r.trade_date, "2026-08-28");
        assert_eq!(r.main_net_inflow, "5", "r0_net 2 + r1_net 3");
        assert_eq!(
            r.main_net_inflow_rate, "2",
            "(2+3)/(100+50+50+50) = 2%, percent unit matches historical f184"
        );
        assert_eq!(r.super_large_net, "2");
        assert_eq!(r.large_net, "3");
        assert_eq!(r.medium_net, "4");
        assert_eq!(r.small_net, "5");
        assert_eq!(r.update_date, today());
    }

    #[test]
    fn parse_sina_row_zero_denominator_yields_zero_rate() {
        let row = serde_json::json!({
            "opendate": "2026-08-28",
            "trade": "sh600519",
            "num": 1,
            "r0": "0", "r0_ratio": "0", "r0_net": "1",
            "r1": "0", "r1_ratio": "0", "r1_net": "2",
            "r2": "0", "r2_ratio": "0", "r2_net": "3",
            "r3": "0", "r3_ratio": "0", "r3_net": "4",
            "netamount": "10", "ratioamount": "0"
        });
        let r = parse_sina_row("SH600519", &row).expect("valid sina row");
        assert_eq!(r.main_net_inflow, "3");
        assert_eq!(r.main_net_inflow_rate, "0", "3/0 must map to 0");
    }

    #[test]
    fn parse_sina_row_missing_opendate_is_skipped() {
        let row = serde_json::json!({"trade": "sh600519", "r0_net": "1", "r1_net": "2"});
        assert!(parse_sina_row("SH600519", &row).is_none());
    }

    fn flow_row(day: &str) -> FlowRecord {
        FlowRecord {
            symbol: "SH600519".to_string(),
            trade_date: day.to_string(),
            main_net_inflow: String::new(),
            main_net_inflow_rate: String::new(),
            super_large_net: String::new(),
            large_net: String::new(),
            medium_net: String::new(),
            small_net: String::new(),
            update_date: String::new(),
        }
    }

    #[test]
    fn filter_daily_window_keeps_only_rows_newer_than_anchor() {
        let rows = vec![
            flow_row("2026-08-26"),
            flow_row("2026-08-27"),
            flow_row("2026-08-28"),
        ];
        let kept = filter_daily_window(rows, Some("2026-08-27"));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].trade_date, "2026-08-28");
    }

    #[test]
    fn filter_daily_window_without_anchor_keeps_all() {
        let rows = vec![flow_row("2026-01-01"), flow_row("2026-08-28")];
        let kept = filter_daily_window(rows, None);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn finalize_daily_csv_zero_rows_removes_stale_csv() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("RPT_MAIN_MONEY_FLOW.csv");
        std::fs::write(&path, "stale").unwrap();
        let out = finalize_daily_csv(&path, Vec::new()).unwrap();
        assert_eq!(out, path);
        assert!(!path.exists(), "0 new rows must remove the stale CSV");
    }

    #[test]
    fn finalize_daily_csv_zero_rows_without_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("RPT_MAIN_MONEY_FLOW.csv");
        assert!(finalize_daily_csv(&path, Vec::new()).is_ok());
        assert!(!path.exists());
    }

    #[test]
    fn finalize_daily_csv_writes_nonempty_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("RPT_MAIN_MONEY_FLOW.csv");
        let out = finalize_daily_csv(&path, vec![flow_row("2026-08-28")]).unwrap();
        assert_eq!(out, path);
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("2026-08-28"));
    }

    #[test]
    fn in_backfill_range_is_inclusive_on_both_ends() {
        assert!(in_backfill_range("2026-08-27", "2026-08-27", "2026-08-28"));
        assert!(in_backfill_range("2026-08-28", "2026-08-27", "2026-08-28"));
        assert!(in_backfill_range("2026-08-27", "2026-08-27", "2026-08-27"));
        assert!(!in_backfill_range("2026-08-26", "2026-08-27", "2026-08-28"));
        assert!(!in_backfill_range("2026-08-29", "2026-08-27", "2026-08-28"));
    }

    // ── Adversarial: backfill per-symbol retry (#342) ────────────────────
    //
    // RED rationale (plan fix-backfill-retry-import-history #342): backfill()
    // today has NO per-symbol retry — the first transport error propagates
    // via `?` (main_flow.rs backfill loop) and the error names neither the
    // symbol nor the attempt count. The fix must retry SINA_BACKFILL_RETRIES
    // (3) times with the daily-path backoff and fail strict with a symbol-
    // naming error (BackfillSymbolFailed) after exhaustion.
    //
    // Deterministic failure injection WITHOUT any HTTP mock: wreq 0.16.1
    // enables the system proxy by default (ClientBuilder auto_sys_proxy:
    // true, client.rs build()) and HttpClient::new() does NOT call
    // no_proxy() — so HTTPS_PROXY pointing at a just-released local port
    // makes every request fail instantly with connection refused (no DNS,
    // no external network, no timeouts).
    //
    // Wall-clock cost: this is an end-to-end test through the production
    // backfill() path, so it sleeps the real 2s/4s backoff (~6s) before
    // asserting exhaustion. The backoff floor itself is covered with an
    // injected short backoff in retry_sina_backfill_exponential_backoff_sequence
    // (10ms/20ms — not slept here); this test keeps the production constants
    // on purpose, it is the strict-abort + error-naming contract.

    #[tokio::test]
    async fn backfill_permanent_failure_names_symbol_and_attempts() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().expect("tempdir");

        // Dead local port: bind then release.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
            l.local_addr().expect("local addr").port()
        };
        let proxy = format!("http://127.0.0.1:{port}");
        let keys = [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
            "COMPASS_CSV_DIR",
        ];
        let saved: Vec<(String, Option<std::ffi::OsString>)> = keys
            .iter()
            .map(|k| (k.to_string(), std::env::var_os(k)))
            .collect();
        unsafe {
            std::env::set_var("HTTPS_PROXY", &proxy);
            std::env::set_var("https_proxy", &proxy);
            std::env::set_var("HTTP_PROXY", &proxy);
            std::env::set_var("http_proxy", &proxy);
            std::env::set_var("ALL_PROXY", &proxy);
            std::env::set_var("all_proxy", &proxy);
            // Neutralise any ambient NO_PROXY (e.g. "localhost,127.0.0.1")
            // that would bypass the dead proxy and hit the real network.
            std::env::set_var("NO_PROXY", "");
            std::env::set_var("no_proxy", "");
            // Keep the CSV output inside the temp dir (csv_dir() creates it).
            std::env::set_var("COMPASS_CSV_DIR", tmp.path());
        }

        let err = backfill("2026-08-03", "2026-08-25", Some(&["SH600519".to_string()]))
            .await
            .expect_err("backfill must fail after retry exhaustion");

        // Plan #342 contract: strict failure naming the symbol and the
        // attempt count. RED today: bare HTTP error with neither.
        let msg = err.to_string();
        assert!(
            msg.contains("SH600519") && msg.contains("3 attempts"),
            "backfill error must name the symbol and 3-attempt exhaustion, got: {msg:?}"
        );

        // Strict abort: no partial CSV may be written on failure (write_csv
        // must stay after the per-symbol loop).
        let csv_file = tmp.path().join("RPT_MAIN_MONEY_FLOW_backfill.csv");
        assert!(
            !csv_file.exists(),
            "no partial backfill CSV may be written on failure"
        );

        // Restore the environment so later tests do not inherit the dead
        // proxy (success path only; a failure aborts the process anyway).
        for (k, v) in saved {
            unsafe {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    // ── Adversarial: #342 interface-level tests (phase 2, SHA 64ef76c) ────
    //
    // These target the plan-declared interface that landed with the skeleton
    // commit (SINA_BACKFILL_RETRIES / retry_sina_backfill /
    // extract_backfill_window). The runner and extractor currently have
    // `unimplemented!()` bodies, so the retry_* tests RED as panics until the
    // behavior lands; the pure-signature test passes immediately. They use
    // short injected backoffs (2ms/10ms) so the production 2s/4s sequence is
    // never actually slept through in tests.

    use std::time::{Duration, Instant};

    #[test]
    fn sina_backfill_retries_constant_is_three() {
        assert_eq!(
            SINA_BACKFILL_RETRIES, 3,
            "plan #342: retry count must match the daily-window policy"
        );
        // Rate-limit invariant (#342): retry sleeps (2s/4s) must stay >= the
        // per-symbol min interval because the throttle is only re-armed on the
        // next symbol's acquire, not between retries of one symbol — see
        // backfill(). Stops a future constant tweak from silently breaking the
        // lower bound.
        assert!(
            SINA_BACKFILL_BACKOFF >= SINA_MIN_INTERVAL,
            "backoff must not dip below the per-symbol min interval"
        );
    }

    #[tokio::test]
    async fn retry_sina_backfill_succeeds_after_transient_errors() {
        // The retry runner takes FnMut() -> Fut with no lifetime bound, so a
        // plain `async { calls += 1 }` cannot capture `&mut` locals (the
        // future would escape the closure body). Count via an Rc<Cell> that
        // is moved into each future instead.
        let calls = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let mut op = {
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    let n = calls.get() + 1;
                    calls.set(n);
                    if n < 3 {
                        Err(CollectError::InvalidInput("transient".into()))
                    } else {
                        Ok(Vec::new())
                    }
                }
            }
        };
        let rows = retry_sina_backfill("SH600519", 3, Duration::from_millis(2), &mut op)
            .await
            .expect("retry must succeed on the 3rd attempt");
        assert_eq!(
            calls.get(),
            3,
            "op must be invoked exactly once per attempt"
        );
        assert!(rows.is_empty(), "empty success window must propagate as Ok");
    }

    #[tokio::test]
    async fn retry_sina_backfill_exhaustion_names_symbol() {
        let result = retry_sina_backfill("SH600519", 3, Duration::from_millis(2), || async {
            Err(CollectError::InvalidInput("boom".into()))
        })
        .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("3 failing attempts must surface as Err, got Ok"),
        };
        match &err {
            CollectError::BackfillSymbolFailed {
                symbol,
                attempts: 3,
                reason,
            } => {
                assert_eq!(symbol, "SH600519", "exhaustion must name the symbol");
                assert!(
                    reason.contains("boom"),
                    "the underlying error text must be forwarded, got: {reason:?}"
                );
            }
            other => panic!(
                "exhaustion must surface BackfillSymbolFailed{{symbol, attempts: 3}}, got: {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn retry_sina_backfill_exponential_backoff_sequence() {
        let t0 = Instant::now();
        let result = retry_sina_backfill("SH600519", 3, Duration::from_millis(10), || async {
            Err(CollectError::InvalidInput("boom".into()))
        })
        .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("must exhaust, got Ok"),
        };
        assert!(matches!(err, CollectError::BackfillSymbolFailed { .. }));
        // Known blind spot (documented, not locked): the elapsed floor only
        // proves >= 10+20ms of total waiting, not the exact sequence (a single
        // 30ms sleep or an extra post-exhaustion sleep would also pass), and
        // the "no sleep after the last failure" guarantee is not asserted.
        // Locking the exact sequence would need tokio time injection; the
        // production 2s/4s formula is kept identical to the daily window.
        assert!(
            t0.elapsed() >= Duration::from_millis(30),
            "exponential backoff must sleep 10+20ms before exhaustion, elapsed: {:?}",
            t0.elapsed()
        );
    }

    #[tokio::test]
    async fn retry_sina_backfill_rejects_zero_attempts() {
        // Lock the attempts == 0 guard: the op must never run and the runner
        // must surface InvalidInput (not a lying unreachable! retry count).
        let calls = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let mut op = {
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    calls.set(calls.get() + 1);
                    Ok(Vec::new())
                }
            }
        };
        let result = retry_sina_backfill("SH600519", 0, Duration::from_millis(2), &mut op).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("attempts == 0 must be rejected, got Ok"),
        };
        assert!(
            matches!(err, CollectError::InvalidInput(_)),
            "expected InvalidInput, got: {err:?}"
        );
        assert_eq!(
            calls.get(),
            0,
            "op must never be invoked with attempts == 0"
        );
    }

    // ── Requirement: #342 extract_backfill_window (pure, no network) ─────
    //
    // Acceptance contract (plan fix-backfill-retry-import-history #342 +
    // issue #342): the per-symbol Sina backfill page parser keeps only rows
    // dated inside the inclusive [start, end] window, drops the rows that
    // parse_sina_row rejects (missing/empty/non-string opendate), and stamps
    // the passed-in symbol on every record. Non-array bodies yield an empty
    // window. RED: the body at main_flow.rs:201 is unimplemented!().

    /// Minimal valid Sina lscjfb row: fixed r0..r3 = 100 each (denominator
    /// 400), so main_net_inflow = r0_net + r1_net and
    /// rate = (r0_net + r1_net) / 400 * 100.
    fn sina_row(day: &str, r0_net: &str, r1_net: &str) -> serde_json::Value {
        serde_json::json!({
            "opendate": day,
            "trade": "sh600519",
            "num": 1,
            "r0": "100", "r0_net": r0_net,
            "r1": "100", "r1_net": r1_net,
            "r2": "100", "r2_net": "0",
            "r3": "100", "r3_net": "0",
        })
    }

    #[test]
    fn extract_backfill_window_keeps_rows_in_range() {
        let data = serde_json::json!([
            sina_row("2026-08-24", "1", "2"), // before start
            sina_row("2026-08-25", "3", "4"), // start
            sina_row("2026-08-26", "5", "6"),
            sina_row("2026-08-27", "7", "8"),  // end
            sina_row("2026-08-28", "9", "10"), // after end
        ]);
        let rows = extract_backfill_window("SH600519", &data, "2026-08-25", "2026-08-27");
        assert_eq!(rows.len(), 3, "only in-range rows survive");
        let mut days: Vec<&str> = rows.iter().map(|r| r.trade_date.as_str()).collect();
        days.sort_unstable();
        assert_eq!(days, ["2026-08-25", "2026-08-26", "2026-08-27"]);
    }

    #[test]
    fn extract_backfill_window_parses_values_with_sina_semantics() {
        let data = serde_json::json!([sina_row("2026-08-26", "2", "3")]);
        let rows = extract_backfill_window("SH600519", &data, "2026-08-25", "2026-08-27");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trade_date, "2026-08-26");
        assert_eq!(rows[0].main_net_inflow, "5", "r0_net 2 + r1_net 3");
        assert_eq!(
            rows[0].main_net_inflow_rate, "1.25",
            "(2+3)/(100+100+100+100) × 100 = 1.25%"
        );
    }

    #[test]
    fn extract_backfill_window_skips_bad_rows() {
        // Missing / empty / non-string opendate and a non-object element:
        // parse_sina_row rejects all of them — the window must drop them
        // rather than panic or emit garbage rows.
        let data = serde_json::json!([
            {"trade": "sh600519", "r0_net": "1", "r1_net": "2"},   // no opendate
            {"opendate": "", "r0_net": "1", "r1_net": "2"},        // empty date
            {"opendate": 20260826, "r0_net": "1", "r1_net": "2"},  // non-string
            "not-an-object",
            sina_row("2026-08-26", "1", "2"),                      // valid
        ]);
        let rows = extract_backfill_window("SH600519", &data, "2026-08-25", "2026-08-27");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trade_date, "2026-08-26");
    }

    #[test]
    fn extract_backfill_window_stamps_passed_symbol() {
        let data = serde_json::json!([sina_row("2026-08-26", "1", "2"),]);
        // #342 原始故障 symbol（bj920837）。
        let rows = extract_backfill_window("BJ920837", &data, "2026-08-25", "2026-08-27");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].symbol, "BJ920837",
            "record must carry the fetched symbol, not the row's `trade` field"
        );
    }

    #[test]
    fn extract_backfill_window_non_array_returns_empty() {
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!("text"),
            serde_json::Value::Bool(true),
        ] {
            assert!(
                extract_backfill_window("SH600519", &value, "2026-08-25", "2026-08-27").is_empty(),
                "non-array page body must yield an empty window, got {value:?}"
            );
        }
    }

    // ── Adversarial: #348 active-symbol filtering + NULL-row guard ────────
    //
    // RED rationale (plan fix-mainflow-null-rows-filter #348): neither
    // active_symbols_sql / parse_symbol_csv / filter_active_symbols /
    // active_symbols exist yet — main_flow.rs:397 backfill_symbols() is the
    // unfiltered predecessor. This batch therefore REDs as a compile-time
    // "interface not landed yet" failure; the two runtime-behaviour tests
    // (backfill all-inactive error message, import NULL-row guard) RED as
    // assertion failures once the module compiles, because the current code
    // still emits the misleading "no symbols to fetch" and imports NULL rows.
    //
    // Dolt-backed tests follow testing.md (dolt init + --data-dir on a
    // self-contained tempdir) and skip gracefully when the dolt CLI is not on
    // PATH (same policy as crates/compass-data/benches/dolt_bench.rs).

    use std::collections::HashSet;

    fn active_set(symbols: &[&str]) -> HashSet<String> {
        symbols.iter().map(|s| s.to_string()).collect()
    }

    /// RAII env override that restores prior values even on panic: a failing
    /// assertion must not poison subsequent tests with a dead proxy or a
    /// temp COMPASS_DATA_DIR.
    struct EnvGuard(Vec<(String, Option<std::ffi::OsString>)>);

    impl EnvGuard {
        fn set(keys: &[(&str, &str)]) -> EnvGuard {
            let saved: Vec<(String, Option<std::ffi::OsString>)> = keys
                .iter()
                .map(|(k, v)| {
                    let prev = std::env::var_os(k);
                    unsafe { std::env::set_var(k, v) };
                    (k.to_string(), prev)
                })
                .collect();
            EnvGuard(saved)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.0 {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }

    fn dolt_available() -> bool {
        std::process::Command::new("dolt")
            .arg("version")
            .output()
            .is_ok()
    }

    fn setup_dolt(dir: &std::path::Path) {
        // Identity is configured with `--local` inside the tempdir repo so a
        // test run never rewrites the host's global dolt config (review MED-1).
        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dir)
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            out.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        for (key, val) in [
            ("user.email", "admainflow@compass.local"),
            ("user.name", "AdMainFlowTest"),
        ] {
            let out = std::process::Command::new("dolt")
                .current_dir(dir)
                .arg("config")
                .arg("--local")
                .arg("--add")
                .arg(key)
                .arg(val)
                .output()
                .expect("dolt config");
            assert!(out.status.success(), "dolt config {key} failed");
        }
    }

    fn dolt_sql(dir: &std::path::Path, sql: &str) {
        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dir)
            .arg("sql")
            .arg("-q")
            .arg(sql)
            .output()
            .expect("dolt sql");
        assert!(
            out.status.success(),
            "dolt sql failed: {}\nsql: {sql}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn csv_last_cell(output: &str) -> String {
        output
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string()
    }

    // ── Contract 1: active_symbols_sql (pure) ─────────────────────────────

    /// The varchar list_date values carry a ` HH:MM:SS` suffix, so a bare
    /// lexicographic `list_date <= '<end>'` mis-ranks dates inside the same
    /// year (e.g. '2026-08-19 00:00:00' vs '2026-08-28' string-compares
    /// GREATER). The SQL must CAST list_date to DATE, quote both date
    /// literals, AND-join the two boundary groups (parens are load-bearing:
    /// OR binds looser than AND) and end in ORDER BY symbol.
    #[test]
    fn active_symbols_sql_casts_list_date_and_quotes_iso_dates() {
        let sql = active_symbols_sql("2026-08-19", "2026-08-28");
        let compact: String = sql.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains("SELECTsymbolFROMstock_basic"),
            "query must scan stock_basic: {sql}"
        );
        assert!(
            compact.contains("list_dateISNULL"),
            "NULL list_date rows must be included: {sql}"
        );
        assert!(
            compact.contains("CAST(list_dateASDATE)<='2026-08-28'"),
            "list_date (varchar, has time suffix) must be CAST to DATE — a bare \
             lexicographic comparison is a date-ordering bug; end must be quoted: {sql}"
        );
        assert!(
            compact.contains("delist_dateISNULL"),
            "NULL delist_date rows must be included: {sql}"
        );
        assert!(
            compact.contains("delist_date>='2026-08-19'"),
            "delist_date must be bounded by the quoted start: {sql}"
        );
        assert!(
            compact.contains(")AND("),
            "the two boundary groups must be AND-joined as a unit (missing \
             parens silently widen the filter to OR semantics): {sql}"
        );
        assert!(
            compact.contains("ORDERBYsymbol"),
            "output must be ordered by symbol: {sql}"
        );
    }

    /// Single-day window (run()'s (today, today) anchor): both bounds must
    /// still appear — a "same date" shortcut that drops one bound would make
    /// the daily run collect delisted or not-yet-listed symbols.
    #[test]
    fn active_symbols_sql_single_day_window_bounds_both_sides() {
        let sql = active_symbols_sql("2026-08-28", "2026-08-28");
        let compact: String = sql.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains("CAST(list_dateASDATE)<='2026-08-28'"),
            "{sql}"
        );
        assert!(compact.contains("delist_date>='2026-08-28'"), "{sql}");
    }

    /// Parameter placement: list_date is bounded by END, delist_date by START.
    /// A swapped implementation silently widens/narrows the active window.
    #[test]
    fn active_symbols_sql_uses_end_for_list_and_start_for_delist() {
        let sql = active_symbols_sql("2026-08-05", "2026-08-28");
        let compact: String = sql.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.contains("CAST(list_dateASDATE)<='2026-08-28'"),
            "list_date must be bounded by end: {sql}"
        );
        assert!(
            compact.contains("delist_date>='2026-08-05'"),
            "delist_date must be bounded by start: {sql}"
        );
    }

    // ── Contract 3: filter_active_symbols (pure) ──────────────────────────

    /// Input order + duplicates + active-set extras must not leak: the output
    /// is a filtered PROJECTION of the input, not of the active set.
    #[test]
    fn filter_active_symbols_keeps_input_order_duplicates_and_ignores_extra() {
        let input = vec![
            "SH600519".to_string(),
            "SZ000001".to_string(),
            "SH600519".to_string(),
            "BJ920000".to_string(),
        ];
        let active = active_set(&["SH600519", "SZ000001", "SH600000"]);
        assert_eq!(
            filter_active_symbols(&input, &active),
            vec!["SH600519", "SZ000001", "SH600519"],
            "input order and duplicates must survive; active-only symbols must not appear"
        );
    }

    /// Empty sides: empty input → empty output; all-inactive input → empty
    /// output (the caller then surfaces the plan's actionable error).
    #[test]
    fn filter_active_symbols_empty_input_or_all_inactive_is_empty() {
        let active = active_set(&["SH600519"]);
        assert!(filter_active_symbols(&[], &active).is_empty());
        let inactive = vec!["SZ000001".to_string(), "BJ920000".to_string()];
        assert!(filter_active_symbols(&inactive, &active).is_empty());
    }

    /// Case/whitespace semantics locked to the pre-#348 backfill: symbols are
    /// used exactly as provided (no trim, no uppercase) — `daima()` lower-
    /// cases only the Sina query key, never the symbol identity. A `contains`
    /// on an unnormalised "sh600519" must NOT match "SH600519".
    #[test]
    fn filter_active_symbols_matches_exact_case_and_no_whitespace_normalization() {
        let active = active_set(&["SH600519"]);
        let stray = vec![
            "sh600519".to_string(),
            " SH600519".to_string(),
            "SH600519 ".to_string(),
        ];
        assert!(
            filter_active_symbols(&stray, &active).is_empty(),
            "existing backfill never normalizes symbols — exact match is the #348 semantics"
        );
    }

    /// Scale smoke (no wall-clock assertion): 50k symbols must neither panic
    /// nor corrupt order/count — guards an accidental O(n²)/recursive shape
    /// from silently degrading the 5.9k-symbol production query.
    #[test]
    fn filter_active_symbols_50k_symbols_keeps_shape_without_panic() {
        let symbols: Vec<String> = (0..50_000).map(|i| format!("SH{i:06}")).collect();
        let active: HashSet<String> = (0..50_000)
            .step_by(2)
            .map(|i| format!("SH{i:06}"))
            .collect();
        let out = filter_active_symbols(&symbols, &active);
        assert_eq!(out.len(), 25_000);
        assert_eq!(out[0], "SH000000");
        assert_eq!(out[24_999], "SH049998");
    }

    // ── Contract 2: parse_symbol_csv (pure) ───────────────────────────────

    /// Header "symbol" skipped, blank lines and the literal `NULL` text line
    /// (Dolt CSV's missing-value shape) filtered — a bare "filter empty"
    /// extraction lets `NULL` leak into the symbol list as a fetch target.
    #[test]
    fn parse_symbol_csv_skips_header_blank_and_null_text_lines() {
        let out = "symbol\nSH600519\n\nSZ000001\nNULL\nBJ920000\n";
        assert_eq!(
            parse_symbol_csv(out),
            vec!["SH600519", "SZ000001", "BJ920000"]
        );
    }

    /// CRLF line endings (Windows CSV) and inline padding must both be
    /// absorbed — data rows must arrive trimmed and de-CR'd.
    #[test]
    fn parse_symbol_csv_trims_crlf_and_inline_whitespace() {
        let out = "symbol\r\n  SH600519  \r\nSZ000001\r\n";
        assert_eq!(parse_symbol_csv(out), vec!["SH600519", "SZ000001"]);
    }

    /// Empty output and header-only output must both yield an empty symbol
    /// list (never a spurious `[symbol]` or a panic); the final line without a
    /// trailing newline must still be kept.
    #[test]
    fn parse_symbol_csv_empty_and_header_only_inputs() {
        assert!(parse_symbol_csv("").is_empty());
        assert!(parse_symbol_csv("symbol\n").is_empty());
        assert_eq!(
            parse_symbol_csv("symbol\nSH600519"),
            vec!["SH600519"],
            "final line without trailing newline must be kept"
        );
    }

    // ── Contract 4/5: active_symbols + backfill error paths ───────────────

    /// No `.dolt` (test/dev env) keeps the plan's SH600519 fallback — this is
    /// the determinism anchor of the backfill error tests below.
    #[tokio::test]
    async fn active_symbols_falls_back_to_sh600519_without_dolt() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(&[(
            "COMPASS_DATA_DIR",
            tmp.path().to_str().expect("utf8 temp path"),
        )]);
        assert_eq!(
            active_symbols("2026-08-19", "2026-08-28").await.unwrap(),
            vec!["SH600519".to_string()],
            "plan #348: missing .dolt must keep the SH600519 fallback"
        );
    }

    /// Some(s) fully filtered before the fetch loop: the error must surface
    /// the cause (outside the active window), the window and the requested
    /// count — and MUST NOT regress to the misleading "no symbols to fetch".
    /// No .dolt forces the SH600519 fallback, so the three requested symbols
    /// are all inactive; a dead local proxy makes a buggy implementation
    /// (filtering after fetch) fail instantly instead of hitting Sina.
    #[tokio::test]
    async fn backfill_all_requested_inactive_errors_with_window_and_count() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
            l.local_addr().expect("local addr").port()
        };
        let proxy = format!("http://127.0.0.1:{port}");
        let _env = EnvGuard::set(&[
            ("HTTPS_PROXY", &proxy),
            ("https_proxy", &proxy),
            ("HTTP_PROXY", &proxy),
            ("http_proxy", &proxy),
            ("ALL_PROXY", &proxy),
            ("all_proxy", &proxy),
            ("NO_PROXY", ""),
            ("no_proxy", ""),
            (
                "COMPASS_DATA_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
            (
                "COMPASS_CSV_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
        ]);

        let symbols = vec![
            "SZ000001".to_string(),
            "SH600000".to_string(),
            "BJ920000".to_string(),
        ];
        let err = backfill("2026-08-19", "2026-08-28", Some(&symbols))
            .await
            .expect_err("all-inactive symbols must be rejected");
        assert!(
            matches!(err, CollectError::InvalidInput(_)),
            "must be InvalidInput, got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            !msg.contains("no symbols to fetch"),
            "the misleading pre-#348 message must not come back: {msg}"
        );
        assert!(
            msg.contains("outside the active window"),
            "must explain the actual cause: {msg}"
        );
        assert!(
            msg.contains("2026-08-19") && msg.contains("2026-08-28"),
            "must name the window: {msg}"
        );
        assert!(
            msg.contains("3 requested"),
            "must name the requested count: {msg}"
        );
        let csv_file = tmp.path().join("RPT_MAIN_MONEY_FLOW_backfill.csv");
        assert!(
            !csv_file.exists(),
            "the error must fire before any fetch/CSV write"
        );
    }

    /// Injection-shaped date (`' OR '1'='1`): the backfill entry guard
    /// (`NaiveDate::parse_from_str`, chrono rejects trailing input) must
    /// reject it as InvalidDate before any Dolt/network/CSV access.
    #[tokio::test]
    async fn backfill_quote_shaped_start_rejected_before_any_io() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(&[
            (
                "COMPASS_DATA_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
            (
                "COMPASS_CSV_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
        ]);
        let symbols = vec!["SH600519".to_string()];
        let err = backfill("2026-08-28' OR '1'='1", "2026-08-28", Some(&symbols))
            .await
            .expect_err("a quote-shaped date must be rejected at the entry guard");
        assert!(
            matches!(err, CollectError::InvalidDate { ref label, .. } if label == "start"),
            "must be InvalidDate for start, got {err:?}"
        );
    }

    // ── Dolt-backed: real SQL semantics (testing.md tempdir pattern) ──────

    /// Boundary rows against a REAL Dolt repo (decision 4 verbatim):
    /// list_date NULL/''/==end/end+1, delist_date NULL/==start/start-1/end+1.
    /// The assertion is the exact sorted result — a wrong cast, a < vs <=
    /// off-by-one, or an ORDER BY omission all change this set.
    #[tokio::test]
    async fn active_symbols_dolt_query_respects_list_delist_boundaries() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        if !dolt_available() {
            eprintln!("skip: dolt CLI not on PATH");
            return;
        }
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE stock_basic \
             (symbol VARCHAR(20) PRIMARY KEY, list_date VARCHAR(20), delist_date DATE)",
        );
        dolt_sql(
            tmp.path(),
            "INSERT INTO stock_basic (symbol, list_date, delist_date) VALUES \
             ('SH600001', NULL, NULL), \
             ('SH600002', '', NULL), \
             ('SH600003', '2026-08-19 00:00:00', NULL), \
             ('SH600004', '2026-08-28 00:00:00', NULL), \
             ('SH600005', '2026-08-29 00:00:00', NULL), \
             ('SH600006', NULL, '2026-08-18'), \
             ('SH600007', NULL, '2026-08-19'), \
             ('SH600008', NULL, '2026-08-29'), \
             ('SH600009', '2026-08-19 00:00:00', '2026-08-29')",
        );
        let _env = EnvGuard::set(&[(
            "COMPASS_DATA_DIR",
            tmp.path().to_str().expect("utf8 temp path"),
        )]);
        let got = active_symbols("2026-08-19", "2026-08-28")
            .await
            .expect("active query on seeded repo");
        assert_eq!(
            got,
            vec![
                "SH600001", "SH600002", "SH600003", "SH600004", "SH600007", "SH600008", "SH600009",
            ],
            "cast-on-list_date, empty-string list_date and inclusive delist \
             bounds must each hold against real Dolt semantics"
        );
    }

    /// Empty stock_basic (Dolt anomaly): active_symbols must Err (not return
    /// an empty Ok that a caller would treat as no-op), and backfill(None)
    /// must propagate the error before touching the fetch loop.
    #[tokio::test]
    async fn active_symbols_empty_repo_errors_and_backfill_none_propagates_it() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        if !dolt_available() {
            eprintln!("skip: dolt CLI not on PATH");
            return;
        }
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE stock_basic \
             (symbol VARCHAR(20) PRIMARY KEY, list_date VARCHAR(20), delist_date DATE)",
        );
        let _env = EnvGuard::set(&[
            (
                "COMPASS_DATA_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
            (
                "COMPASS_CSV_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
        ]);

        let err = active_symbols("2026-08-19", "2026-08-28")
            .await
            .expect_err("empty stock_basic must be an error, not Ok(empty)");
        let msg = err.to_string();
        assert!(matches!(err, CollectError::InvalidInput(_)), "got {err:?}");
        assert!(
            msg.contains("no symbols"),
            "must name the empty source, got: {msg}"
        );

        let err2 = backfill("2026-08-19", "2026-08-28", None)
            .await
            .expect_err("None path must propagate the empty active-set error");
        assert!(
            matches!(err2, CollectError::InvalidInput(_)),
            "None path must not silently no-op, got {err2:?}"
        );
    }

    /// The import guard end-to-end: a CSV with (a) a valid row, (b) a NULL
    /// main_net_inflow row for an in-stock_basic symbol, (c) a non-null row
    /// for a symbol NOT in stock_basic. Only (a) may land in
    /// capital_main_flow; NULL-inflow count must be 0. A text-only guard
    /// (e.g. on the wrong branch or OR-joined) fails these assertions.
    #[tokio::test]
    async fn import_to_dolt_guard_blocks_null_and_unknown_rows() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        if !dolt_available() {
            eprintln!("skip: dolt CLI not on PATH");
            return;
        }
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE stock_basic \
             (symbol VARCHAR(20) PRIMARY KEY, list_date VARCHAR(20), delist_date DATE)",
        );
        dolt_sql(
            tmp.path(),
            "INSERT INTO stock_basic (symbol, list_date, delist_date) VALUES \
             ('SH600519', NULL, NULL), ('SH600000', NULL, NULL)",
        );
        dolt_sql(
            tmp.path(),
            "CREATE TABLE data_updates (\
             table_name VARCHAR(50) PRIMARY KEY, last_updated DATE, \
             source VARCHAR(200), row_count BIGINT, last_report_date VARCHAR(30))",
        );
        let csv_path = tmp.path().join("main_flow.csv");
        std::fs::write(
            &csv_path,
            "symbol,trade_date,main_net_inflow,main_net_inflow_rate,\
             super_large_net,large_net,medium_net,small_net,update_date\n\
             SH600519,2026-08-19,5,1,2,3,4,5,2026-09-02\n\
             SH600000,2026-08-19,,1,1,1,1,1,2026-09-02\n\
             SZ000001,2026-08-19,9,1,1,1,1,1,2026-09-02\n",
        )
        .unwrap();
        let _env = EnvGuard::set(&[(
            "COMPASS_DATA_DIR",
            tmp.path().to_str().expect("utf8 temp path"),
        )]);

        let total = import_to_dolt(Some(&csv_path))
            .await
            .expect("import to temp dolt repo");
        assert_eq!(
            total, 1,
            "only the valid in-stock_basic non-null row may be inserted"
        );
        let nulls = crate::dolt::dolt_sql_csv(
            "SELECT COUNT(*) FROM capital_main_flow WHERE main_net_inflow IS NULL",
        )
        .await
        .expect("count null rows");
        assert_eq!(
            csv_last_cell(&nulls),
            "0",
            "the #348 guard must prevent ANY NULL main_net_inflow row"
        );
        let rows =
            crate::dolt::dolt_sql_csv("SELECT symbol FROM capital_main_flow ORDER BY symbol")
                .await
                .expect("list inserted rows");
        assert_eq!(
            csv_last_cell(&rows),
            "SH600519",
            "only the valid row survives the guard"
        );
    }

    // ── Requirement acceptance: #348 active-symbol contract (batch 2) ─────
    //
    // 需求验收 RED（subagent_skwy_requirement_test，门禁第 4 步）。与批次 1
    // （对抗性，c24260d）的分工：对抗性已深度覆盖 SQL 子串断言 / filter 顺序
    // 重复空 / parse CSV header NULL CRLF / 无 .dolt fallback / Some 全不活跃
    // Err 消息 / 注入形日期 / Dolt 边界行 / 空 repo 传播 / 导入守卫端到端。
    // 本批次只补对抗性未锁定的需求验收差异点：
    //   1. active_symbols_sql 的"完整形态"层——plan 契约是"不含换行的单条
    //      SELECT ... ORDER BY symbol"；批次 1 只断言子串片段，未断言整串
    //      无换行 / 开头结尾 / 引号只出现在两个日期字面量上。
    //   2. backfill(Some) 部分命中（1 活跃 + 1 不活跃，不活跃在前）→ 必须
    //      在 fetch 前过滤：只 fetch 活跃交集。批次 1 测的是"全部不活跃 →
    //      Err"（错误路径）；这里是"部分活跃 → 进入 fetch 且只 fetch 过活的"
    //      （happy path 的制造性验证）。制造方式：无 .dolt fallback 活跃集
    //      固定 {SH600519}，请求 [SZ000001, SH600519]；死端口代理让 fetch
    //      必然失败——若过滤未先发生（现状），失败 symbol 是 SZ000001；若
    //      过滤生效，失败 symbol 必为 SH600519（错误是 BackfillSymbolFailed
    //      而非 InvalidInput 也证明走入了 fetch 循环）。

    /// Plan #348: active_symbols_sql must generate ONE single-line SELECT
    /// (no newlines) starting at the stock_basic scan and ending with
    /// ORDER BY symbol, with the dates quoted exactly once each. Batch 1
    /// asserts the WHERE fragments; this locks the whole-string shape the
    /// plan declares ("不含换行的单条 SELECT") — a multi-line formatting
    /// regression or a trailing clause after ORDER BY breaks this test.
    #[test]
    fn active_symbols_sql_is_single_line_select_with_full_shape() {
        let sql = active_symbols_sql("2026-08-19", "2026-08-28");
        let trimmed = sql.trim();
        assert!(
            trimmed.starts_with("SELECT symbol FROM stock_basic WHERE"),
            "must open with the stock_basic scan + WHERE head: {trimmed}"
        );
        assert!(
            trimmed.ends_with("ORDER BY symbol"),
            "plan: a single SELECT ... ORDER BY symbol — nothing may trail: {trimmed}"
        );
        assert!(
            !trimmed.contains('\n') && !trimmed.contains('\r'),
            "plan: single-line SELECT without newlines/CR: {trimmed}"
        );
        assert!(
            trimmed.matches('\'').count() >= 4,
            "the two quoted date literals carry one quote pair each; extra \
             singles are reserved for the empty-string list_date literal \
             (Dolt CAST('') is NULL — decision 4 keeps empty list_date rows, \
             batch-1 boundary test SH600002): {trimmed}"
        );
        assert!(
            trimmed.contains("'2026-08-19'") && trimmed.contains("'2026-08-28'"),
            "both quoted date literals must appear: {trimmed}"
        );
    }

    /// Some(list) partial-active intersection: [SZ000001 (inactive),
    /// SH600519 (active fallback)] must be intersected BEFORE the fetch
    /// loop — a dead local proxy turns the reached fetch into
    /// BackfillSymbolFailed{symbol: SH600519}. A regression that fetches the
    /// raw list first (or filters after fetch) fails on the FIRST requested
    /// symbol (SZ000001) instead; a regression that rejects partial-active
    /// lists errors as InvalidInput. Batch 1 covers the all-inactive Err
    /// message depth (backfill_all_requested_inactive_errors_with_window_and_count);
    /// this test locks the happy-path intersection behavior.
    #[tokio::test]
    async fn backfill_partial_active_reaches_fetch_loop_for_active_only() {
        let _guard = crate::config::ENV_MUTEX.lock().await;
        let tmp = tempfile::tempdir().unwrap();
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
            l.local_addr().expect("local addr").port()
        };
        let proxy = format!("http://127.0.0.1:{port}");
        let _env = EnvGuard::set(&[
            ("HTTPS_PROXY", &proxy),
            ("https_proxy", &proxy),
            ("HTTP_PROXY", &proxy),
            ("http_proxy", &proxy),
            ("ALL_PROXY", &proxy),
            ("all_proxy", &proxy),
            ("NO_PROXY", ""),
            ("no_proxy", ""),
            (
                "COMPASS_DATA_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
            (
                "COMPASS_CSV_DIR",
                tmp.path().to_str().expect("utf8 temp path"),
            ),
        ]);

        // No .dolt: active_symbols yields the plan's fallback {SH600519}.
        // SZ000001 first, so the unfiltered (pre-#348) code path fails the
        // fetch on SZ000001 and would expose an unfiltered list here.
        let symbols = vec!["SZ000001".to_string(), "SH600519".to_string()];
        let err = backfill("2026-08-19", "2026-08-28", Some(&symbols))
            .await
            .expect_err("dead proxy: the active intersection must reach the fetch loop");
        match &err {
            CollectError::BackfillSymbolFailed { symbol, .. } => {
                assert_eq!(
                    symbol.as_str(),
                    "SH600519",
                    "only the active intersection may be fetched — the inactive \
                     SZ000001 must be filtered BEFORE the loop, got {err}"
                );
            }
            other => panic!(
                "a partial-active Some(list) must reach the fetch loop \
                 (BackfillSymbolFailed via the dead proxy), got: {other:?}"
            ),
        }
        let csv_file = tmp.path().join("RPT_MAIN_MONEY_FLOW_backfill.csv");
        assert!(
            !csv_file.exists(),
            "no partial backfill CSV may be written on fetch failure"
        );
    }
}
