//! SEPA CLI entry (epic #139, sub-issue #150).
//!
//! Runs the SEPA scoring engine over the local Parquet main database, prints
//! a TOP-N table (or the market thermometer) and writes the computation
//! results back to the Dolt `compass_data` repo with a locked two-stage
//! write-back (`DELETE` by trade_date + `dolt table import -a`) — never
//! `REPLACE INTO`. The day-level delete makes re-runs idempotent.

use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{NaiveDate, Utc};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::run_sepa;
use compass_strategy::sepa::scoring::DEFAULT_TOP_N;
use compass_types::{MarketThermometer, SepaData, SepaQuery, SepaRow};
use tracing::info;

/// Dolt `compass_data` tables written back by the SEPA CLI.
const COMPUTE_TABLES: [&str; 5] = [
    "technical_factor",
    "industry_factor",
    "capital_factor",
    "final_score",
    "market_temperature",
];

const TECH_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS technical_factor (\
    symbol VARCHAR(20) NOT NULL, \
    trade_date DATE NOT NULL, \
    structure_score DOUBLE, position_score DOUBLE, rs_score DOUBLE, \
    vcp_score DOUBLE, breakout_score DOUBLE, \
    update_date DATE, \
    PRIMARY KEY (symbol, trade_date))";

const INDUSTRY_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS industry_factor (\
    concept_name VARCHAR(50) NOT NULL, \
    trade_date DATE NOT NULL, \
    stock_count INTEGER, \
    gain_score DOUBLE, amount_score DOUBLE, diffusion_score DOUBLE, \
    heat_score DOUBLE, news_score DOUBLE, \
    update_date DATE, \
    PRIMARY KEY (concept_name, trade_date))";

const CAPITAL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capital_factor (\
    symbol VARCHAR(20) NOT NULL, \
    trade_date DATE NOT NULL, \
    volume_price_score DOUBLE, chip_score DOUBLE, big_capital_score DOUBLE, \
    update_date DATE, \
    PRIMARY KEY (symbol, trade_date))";

const FINAL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS final_score (\
    symbol VARCHAR(20) NOT NULL, \
    trade_date DATE NOT NULL, \
    trend_score DOUBLE, theme_score DOUBLE, money_score DOUBLE, \
    pattern_score DOUBLE, risk_score DOUBLE, total_score DOUBLE, \
    `rank` INTEGER, update_date DATE, \
    PRIMARY KEY (symbol, trade_date))";

const TEMPERATURE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS market_temperature (\
    trade_date DATE NOT NULL, \
    score DOUBLE, \
    hs300_trend DOUBLE, zz1000_trend DOUBLE, \
    limit_up_count INTEGER, total_amount DOUBLE, breadth DOUBLE, \
    position_suggestion VARCHAR(20), \
    update_date DATE, \
    PRIMARY KEY (trade_date))";

pub(crate) const UPDATES_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS data_updates (\
    table_name VARCHAR(50) NOT NULL, \
    last_updated DATE NOT NULL, \
    source VARCHAR(200), \
    row_count INT, \
    last_report_date DATE, \
    PRIMARY KEY (table_name))";

/// Run the SEPA scoring engine for `date` (default: latest trading day in the
/// data) and write the FULL computed set back to the Dolt repo at `dolt_dir`.
/// `top` only caps the printed table, never the persisted rows (P0-1
/// regression: `--top` must not truncate the Dolt write-back).
pub fn run_score(
    top: usize,
    date: Option<NaiveDate>,
    reader: &ParquetReader,
    dolt_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let now = match date {
        Some(d) => d,
        None => reader
            .latest_trade_date()?
            .unwrap_or_else(|| Utc::now().date_naive()),
    };
    // usize::MAX = compute the full market set; `top` only slices the print.
    let query = SepaQuery { top_n: usize::MAX };
    let started = std::time::Instant::now();
    let data = run_sepa(&query, reader, now)?;
    let shown = data
        .rows
        .len()
        .min(if top == 0 { DEFAULT_TOP_N } else { top });
    info!(
        matched = data.rows.len(),
        returned = shown,
        elapsed_ms = started.elapsed().as_millis(),
        date = %data.date,
        "sepa score run completed"
    );
    println!("{}", format_top_table(&data.rows[..shown]));
    write_back(dolt_dir, &data, &COMPUTE_TABLES)
}

/// Compute the whole-market thermometer and write it back to the Dolt repo
/// at `dolt_dir`. Only `market_temperature` is written (P0-2 regression:
/// a temperature run must never touch the factor/score tables).
pub fn run_temperature(reader: &ParquetReader, dolt_dir: &Path) -> Result<(), Box<dyn Error>> {
    let now = reader
        .latest_trade_date()?
        .unwrap_or_else(|| Utc::now().date_naive());
    let started = std::time::Instant::now();
    let data = run_sepa(&SepaQuery { top_n: 1 }, reader, now)?;
    let tm = &data.thermometer;
    info!(
        elapsed_ms = started.elapsed().as_millis(),
        date = %data.date,
        "sepa temperature run completed"
    );
    println!(
        "市场温度: {:.1} | 仓位建议: {} | 日期: {}",
        tm.score,
        position_band(tm.position_key),
        data.date
    );
    write_back(dolt_dir, &data, &["market_temperature"])
}

/// Render the TOP-N table as a mono-spaced, `{:.1}`-aligned text table.
pub fn format_top_table(rows: &[SepaRow]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:>4}  {:<8}  {:<12}  {:>6}  {:>6}  {:>6}  {:>6}  {:>6}  {:>6}\n",
        "rank", "代码", "名称", "总分", "趋势", "题材", "资金", "形态", "风险"
    ));
    for row in rows {
        out.push_str(&format!(
            "{:>4}  {:<8}  {:<12}  {:>6.1}  {:>6.1}  {:>6.1}  {:>6.1}  {:>6.1}  {:>6.1}\n",
            row.rank,
            row.symbol,
            row.name,
            row.total_score,
            row.trend,
            row.theme,
            row.capital,
            row.pattern,
            row.risk,
        ));
    }
    out
}

/// Serialize the thermometer as one CSV data row (header excluded).
///
/// The model carries semantic keys and raw values (issue #222); the CSV
/// columns keep the exact pre-i18n values, derived from `value` + `unit_key`
/// with the locked per-unit precision (percent 1 decimal, count integer,
/// trillion yuan as full integer).
fn thermometer_csv_row(tm: &MarketThermometer, date: NaiveDate) -> String {
    let find = |label_key: &str| tm.indicators.iter().find(|i| i.label_key == label_key);
    // Percent values are already in percent units.
    let pct = |label_key: &str| {
        find(label_key)
            .map(|i| format!("{:.6}", i.value))
            .unwrap_or_default()
    };
    // Count values are integer counts carried as f64.
    let count = |label_key: &str| {
        find(label_key)
            .map(|i| format!("{}", i.value.round() as i64))
            .unwrap_or_default()
    };
    // Trillion values are stored as trillion-yuan; the CSV column is yuan.
    let trillion = |label_key: &str| {
        find(label_key)
            .map(|i| format!("{:.6}", i.value * 1e12))
            .unwrap_or_default()
    };
    format!(
        "{date},{:.6},{},{},{},{},{},{},{}",
        tm.score,
        pct("sepa.indicator.hs300_trend"),
        pct("sepa.indicator.zz1000_trend"),
        count("sepa.indicator.limit_up"),
        trillion("sepa.indicator.amount"),
        pct("sepa.indicator.breadth"),
        csv_field(position_band(tm.position_key)),
        Utc::now().date_naive(),
    )
}

/// Map a thermometer position-band i18n key back to the locked CSV band
/// string — the `position_suggestion` column stays a data-neutral value.
fn position_band(position_key: &str) -> &'static str {
    match position_key {
        "sepa.position.full" => "80%-100%",
        "sepa.position.mid" => "40%-70%",
        _ => "0%-20%",
    }
}

/// Quote a CSV field: wrap in double quotes and double inner quotes.
pub(crate) fn csv_field(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Format a double for CSV: up to 6 decimals, no exponent.
pub(crate) fn fmt_double(v: f64) -> String {
    if v == v.trunc() {
        format!("{v:.1}")
    } else {
        format!("{v:.6}")
    }
}

/// Two-stage write-back (epic #139 decision 15): DELETE the target
/// trade_date from the scoped compute tables, then append CSV rows via
/// `dolt table import -a`. Idempotent by construction. `tables` limits the
/// write-back scope (P0-2: `run_temperature` passes only `market_temperature`
/// so factor/score rows from a prior score run survive untouched).
fn write_back(dolt_dir: &Path, data: &SepaData, tables: &[&str]) -> Result<(), Box<dyn Error>> {
    let date = data
        .date
        .parse::<NaiveDate>()
        .map_err(|e| format!("invalid sepa date {:?}: {e}", data.date))?;
    let today = Utc::now().date_naive();

    dolt_sql(dolt_dir, TECH_SCHEMA)?;
    dolt_sql(dolt_dir, INDUSTRY_SCHEMA)?;
    dolt_sql(dolt_dir, CAPITAL_SCHEMA)?;
    dolt_sql(dolt_dir, FINAL_SCHEMA)?;
    dolt_sql(dolt_dir, TEMPERATURE_SCHEMA)?;
    dolt_sql(dolt_dir, UPDATES_SCHEMA)?;

    for table in tables {
        dolt_sql(
            dolt_dir,
            &format!("DELETE FROM {table} WHERE trade_date = '{date}'"),
        )?;
    }

    // Symbols are exchange-prefixed end-to-end (issue #181): write the
    // engine's `row.symbol` through unchanged.
    let symbol_csv = |symbol: &str| symbol.to_string();

    // technical_factor: per-stock trend/pattern sub-scores.
    let tech_csv: String = {
        let mut csv = String::from(
            "symbol,trade_date,structure_score,position_score,rs_score,vcp_score,breakout_score,update_date\n",
        );
        for row in &data.rows {
            let t = &row.details.trend;
            let p = &row.details.pattern;
            csv.push_str(&format!(
                "{},{date},{},{},{},{},{},{}\n",
                csv_field(&symbol_csv(&row.symbol)),
                fmt_double(t.first().map_or(0.0, |f| f.score)),
                fmt_double(t.get(1).map_or(0.0, |f| f.score)),
                fmt_double(t.get(2).map_or(0.0, |f| f.score)),
                fmt_double(p.first().map_or(0.0, |f| f.score)),
                fmt_double(p.get(1).map_or(0.0, |f| f.score)),
                today,
            ));
        }
        csv
    };

    // industry_factor: concept-level aggregation over the ranked rows.
    let ind_csv: String = {
        let mut agg: HashMap<&str, Vec<&SepaRow>> = HashMap::new();
        for row in &data.rows {
            for theme in &row.themes {
                agg.entry(theme.as_str()).or_default().push(row);
            }
        }
        let mut csv = String::from(
            "concept_name,trade_date,stock_count,gain_score,amount_score,diffusion_score,heat_score,news_score,update_date\n",
        );
        let mut names: Vec<&&str> = agg.keys().collect();
        names.sort();
        for name in names {
            let rows = &agg[*name];
            let n = rows.len();
            let avg = |idx: usize| -> f64 {
                let sum: f64 = rows
                    .iter()
                    .filter_map(|r| r.details.theme.get(idx))
                    .map(|f| f.score)
                    .sum();
                sum / n as f64
            };
            let heat: f64 = rows.iter().map(|r| r.theme).sum::<f64>() / n as f64;
            csv.push_str(&format!(
                "{},{date},{},{},{},{},{},{},{}\n",
                csv_field(name),
                n,
                fmt_double(avg(0)),
                fmt_double(avg(1)),
                fmt_double(avg(2)),
                fmt_double(heat),
                fmt_double(avg(3)),
                today,
            ));
        }
        csv
    };

    // capital_factor: per-stock capital sub-scores.
    let cap_csv: String = {
        let mut csv = String::from(
            "symbol,trade_date,volume_price_score,chip_score,big_capital_score,update_date\n",
        );
        for row in &data.rows {
            let c = &row.details.capital;
            csv.push_str(&format!(
                "{},{date},{},{},{},{}\n",
                csv_field(&symbol_csv(&row.symbol)),
                fmt_double(c.first().map_or(0.0, |f| f.score)),
                fmt_double(c.get(1).map_or(0.0, |f| f.score)),
                fmt_double(c.get(2).map_or(0.0, |f| f.score)),
                today,
            ));
        }
        csv
    };

    // final_score: the ranked TOP-N rows as-is.
    let final_csv: String = {
        let mut csv = String::from(
            "symbol,trade_date,trend_score,theme_score,money_score,pattern_score,risk_score,total_score,rank,update_date\n",
        );
        for row in &data.rows {
            csv.push_str(&format!(
                "{},{date},{},{},{},{},{},{},{},{}\n",
                csv_field(&symbol_csv(&row.symbol)),
                fmt_double(row.trend),
                fmt_double(row.theme),
                fmt_double(row.capital),
                fmt_double(row.pattern),
                fmt_double(row.risk),
                fmt_double(row.total_score),
                row.rank,
                today,
            ));
        }
        csv
    };

    let temp_csv: String = {
        let mut csv = String::from(
            "trade_date,score,hs300_trend,zz1000_trend,limit_up_count,total_amount,breadth,position_suggestion,update_date\n",
        );
        csv.push_str(&thermometer_csv_row(&data.thermometer, date));
        csv.push('\n');
        csv
    };

    // Stage CSVs to temp files and append-import (per-table: skip when a
    // table legitimately has no rows, e.g. no concept memberships).
    let staged: [(&str, &str, &str); 5] = [
        ("technical_factor", &tech_csv, "technical_factor.csv"),
        ("industry_factor", &ind_csv, "industry_factor.csv"),
        ("capital_factor", &cap_csv, "capital_factor.csv"),
        ("final_score", &final_csv, "final_score.csv"),
        ("market_temperature", &temp_csv, "market_temperature.csv"),
    ];
    for (table, csv, file) in staged {
        if !tables.contains(&table) {
            continue;
        }
        if csv.lines().count() <= 1 {
            info!(table, date = %data.date, "no rows to write back");
            continue;
        }
        let path = stage_csv(&format!("{date}_{file}"), csv)?;
        let import = dolt_import(dolt_dir, table, &path);
        let _ = std::fs::remove_file(&path);
        import?;
        let row_count = csv.lines().count() - 1;
        dolt_upsert_updates(dolt_dir, table, today, date, row_count)?;
        info!(
            table,
            date = %data.date,
            rows = row_count,
            "sepa table written back to dolt"
        );
    }
    Ok(())
}

/// Run `dolt sql -q <query>`; fail loudly on any subprocess error.
pub(crate) fn dolt_sql(dolt_dir: &Path, query: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new("dolt")
        .arg("--data-dir")
        .arg(dolt_dir)
        .arg("sql")
        .arg("-q")
        .arg(query)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dolt error: {stderr}").into());
    }
    Ok(())
}

/// Run `dolt table import -a --continue <table> <csv>` (append mode).
pub(crate) fn dolt_import(dolt_dir: &Path, table: &str, csv: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("dolt")
        .arg("--data-dir")
        .arg(dolt_dir)
        .arg("table")
        .arg("import")
        .arg("-a")
        .arg("--continue")
        .arg(table)
        .arg(csv)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("dolt import {table} failed: {stderr}{stdout}").into());
    }
    Ok(())
}

/// Stage a CSV into a unique temp file under `compass_sepa_writeback`,
/// returning the created path. The caller must `remove_file` the path
/// after `dolt_import` (shared by `write_back` and backtest write-back).
///
/// Uniqueness: PID + per-process atomic sequence keep paths distinct
/// across nextest processes and within one process (ref #184 — a fixed
/// `{date}_{table}.csv` path raced between parallel test runs). `create_new`
/// (O_EXCL) refuses symlinks another local user could plant in the shared
/// temp dir; EEXIST (PID reuse + stale file) bumps the sequence and retries.
pub(crate) fn stage_csv(stem: &str, csv: &str) -> Result<PathBuf, Box<dyn Error>> {
    let temp_dir = std::env::temp_dir().join("compass_sepa_writeback");
    std::fs::create_dir_all(&temp_dir)?;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    loop {
        let candidate = temp_dir.join(format!(
            "{stem}_{}_{}.csv",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                f.write_all(csv.as_bytes())?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Upsert the data_updates row for one compute table.
pub(crate) fn dolt_upsert_updates(
    dolt_dir: &Path,
    table: &str,
    today: NaiveDate,
    report_date: NaiveDate,
    row_count: usize,
) -> Result<(), Box<dyn Error>> {
    let query = format!(
        "INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) \
         VALUES ('{table}', '{today}', 'compass-data sepa', {row_count}, '{report_date}') \
         ON DUPLICATE KEY UPDATE last_updated='{today}', source='compass-data sepa', \
         row_count={row_count}, last_report_date='{report_date}'"
    );
    dolt_sql(dolt_dir, &query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use chrono::{Datelike, Weekday};

    // -----------------------------------------------------------------------
    // Dolt helpers (mirror import_dolt.rs test pattern)
    // -----------------------------------------------------------------------

    fn setup_dolt(dir: &Path) {
        // No `dolt config --global` here: tests never commit, and the
        // process-global HOME mutation would race with main.rs's ENV_MUTEX
        // tests in the same test binary.
        let init = Command::new("dolt")
            .arg("--data-dir")
            .arg(dir)
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    fn dolt_count(dolt_dir: &Path, table: &str, date: &str) -> i64 {
        let csv = crate::import_dolt::run_dolt_sql_csv(
            dolt_dir,
            &format!("SELECT COUNT(*) AS cnt FROM {table} WHERE trade_date = '{date}'"),
        )
        .expect("count query");
        csv.lines()
            .nth(1)
            .and_then(|l| l.parse::<i64>().ok())
            .expect("parse count")
    }

    /// Regression (ref #184): stage_csv must hand out a distinct path per
    /// call even for the same stem — the old fixed `{end}_backtest_result.csv`
    /// path raced between parallel nextest processes.
    #[test]
    fn stage_csv_returns_distinct_paths_per_call() {
        let stem = "unit_test_stage_csv";
        let p1 = stage_csv(stem, "a,b\n1,2\n").expect("first stage");
        let p2 = stage_csv(stem, "a,b\n3,4\n").expect("second stage");
        assert_ne!(p1, p2, "same stem must not collide, got {p1:?} and {p2:?}");
        assert_eq!(std::fs::read_to_string(&p1).expect("read p1"), "a,b\n1,2\n");
        assert_eq!(std::fs::read_to_string(&p2).expect("read p2"), "a,b\n3,4\n");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    /// Occupied candidate names must be skipped: create_new fails with
    /// EEXIST (stale file / PID reuse), the sequence bumps, and the call
    /// lands on a fresh path. Obstacles are placed with create_new so a
    /// parallel test's staged file is never clobbered.
    #[test]
    fn stage_csv_retries_when_candidate_exists() {
        let stem = "unit_test_stage_csv_retry";
        let temp_dir = std::env::temp_dir().join("compass_sepa_writeback");
        std::fs::create_dir_all(&temp_dir).expect("temp dir");
        let pid = std::process::id();
        let mut obstacles = Vec::new();
        for seq in 0..1024 {
            let p = temp_dir.join(format!("{stem}_{pid}_{seq}.csv"));
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
            {
                let _ = f.write_all(b"occupied");
                obstacles.push(p);
            }
        }
        // Whatever the shared counter holds (< 1024 stage_csv calls in this
        // process), the next call hits an obstacle and retries past the range.
        let p1 = stage_csv(stem, "a,b\n1,2\n").expect("stage after collisions");
        let name1 = p1.file_name().unwrap().to_string_lossy().into_owned();
        let seq1: u64 = name1
            .trim_end_matches(".csv")
            .rsplit('_')
            .next()
            .unwrap()
            .parse()
            .expect("seq");
        assert!(
            seq1 >= 1024,
            "must retry past the occupied range, got {seq1}"
        );
        assert_eq!(std::fs::read_to_string(&p1).expect("read"), "a,b\n1,2\n");
        for p in obstacles {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_file(&p1);
    }

    /// A non-EEXIST open failure (missing parent dir) propagates as Err.
    #[test]
    fn stage_csv_propagates_create_error() {
        let stem = "unit_test/nonexistent/stem";
        let result = stage_csv(stem, "a,b\n1,2\n");
        assert!(result.is_err(), "create_new on missing parent must fail");
    }

    // -----------------------------------------------------------------------
    // Parquet fixture (minimal: daily + basic + one concept member; the rest
    // of the SEPA tables stay absent — run_sepa degrades them to empty vecs)
    // -----------------------------------------------------------------------

    #[derive(Clone)]
    struct TestBar {
        date: String,
        close: f64,
        amount: f64,
    }

    struct TestStock {
        symbol: &'static str,
        name: &'static str,
        exchange: &'static str,
        bars: Vec<TestBar>,
    }

    fn filler_series(end: &str) -> Vec<TestBar> {
        let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
        let mut out = Vec::new();
        for k in 0..300 {
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            let close = if k == 299 { 15.3 } else { 15.0 };
            out.push(TestBar {
                date: day.format("%Y-%m-%d").to_string(),
                close,
                amount: 5.0e8,
            });
            day -= Duration::days(1);
        }
        out.reverse();
        out
    }

    fn build_fixture(dir: &Path) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        let stocks = vec![
            TestStock {
                symbol: "SZ000001",
                name: "平安银行",
                exchange: "SZ",
                bars: filler_series("2026-07-31"),
            },
            TestStock {
                symbol: "SH600001",
                name: "测试甲",
                exchange: "SH",
                bars: filler_series("2026-07-31"),
            },
            TestStock {
                symbol: "SH600002",
                name: "测试乙",
                exchange: "SH",
                bars: filler_series("2026-07-31"),
            },
        ];
        for s in &stocks {
            for b in &s.bars {
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        s.symbol,
                        b.date.as_str(),
                        b.close - 1.0,
                        b.close + 0.1,
                        b.close - 0.1,
                        b.close,
                        b.close,
                        1.0e6,
                        b.amount
                    ],
                )
                .expect("insert daily");
            }
        }
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            dir.join("stock_daily.parquet").display()
        ))
        .expect("copy daily");

        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .expect("create basic");
        for s in &stocks {
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, '2010-01-01', NULL, '主板', ?, 1.0e9, '测试', NULL)",
                duckdb::params![s.symbol, s.name, s.exchange, s.name],
            )
            .expect("insert basic");
        }
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            dir.join("stock_basic.parquet").display()
        ))
        .expect("copy basic");

        conn.execute_batch(
            "CREATE TABLE concept_member (concept_code VARCHAR, symbol VARCHAR, concept_name VARCHAR, update_date DATE);",
        )
        .expect("create concept_member");
        conn.execute(
            "INSERT INTO concept_member VALUES ('BK1000', 'SZ000001', 'AI概念', '2026-07-31')",
            [],
        )
        .expect("insert concept_member");
        conn.execute_batch(&format!(
            "COPY concept_member TO '{}' (FORMAT PARQUET)",
            dir.join("concept_member.parquet").display()
        ))
        .expect("copy concept_member");
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn format_top_table_has_header_and_ranked_rows() {
        let rows = vec![
            SepaRow {
                symbol: "SZ000001".to_string(),
                name: "平安银行".to_string(),
                rank: 1,
                total_score: 86.44,
                trend: 30.0,
                theme: 25.0,
                capital: 16.0,
                pattern: 15.0,
                risk: -1.5,
                industry: "测试".to_string(),
                industry_en: None,
                themes: vec!["AI概念".to_string()],
                latest_price: 25.0,
                change_pct: 2.3,
                details: compass_types::SepaDetails {
                    trend: vec![],
                    theme: vec![],
                    capital: vec![],
                    pattern: vec![],
                    risk: vec![],
                },
            },
            SepaRow {
                symbol: "SH600001".to_string(),
                name: "测试甲".to_string(),
                rank: 2,
                total_score: 60.0,
                trend: 20.0,
                theme: 15.0,
                capital: 10.0,
                pattern: 10.0,
                risk: 0.0,
                industry: "测试".to_string(),
                industry_en: None,
                themes: vec![],
                latest_price: 15.3,
                change_pct: 2.0,
                details: compass_types::SepaDetails {
                    trend: vec![],
                    theme: vec![],
                    capital: vec![],
                    pattern: vec![],
                    risk: vec![],
                },
            },
        ];
        let table = format_top_table(&rows);
        assert!(table.contains("代码"), "header: {table}");
        assert!(table.contains("SZ000001"), "rank 1 row: {table}");
        assert!(table.contains("平安银行"), "name: {table}");
        assert!(table.contains("86.4"), "one decimal: {table}");
        assert!(table.contains("-1.5"), "risk sign: {table}");
    }

    #[test]
    fn run_score_writes_five_tables_and_prints_table() {
        // `dolt` reads the process-global HOME: hold ENV_MUTEX so main.rs
        // HOME-mutating tests cannot delete it mid-spawn.
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));
        run_score(50, date, &reader, dolt_tmp.path()).expect("run_score");

        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"), 3);
        // Written symbols must be the exchange-prefixed forms — never a
        // double-prefixed "SHSZ000001"-style artifact of prefix concatenation.
        let csv = crate::import_dolt::run_dolt_sql_csv(
            dolt_tmp.path(),
            "SELECT symbol FROM final_score WHERE trade_date = '2026-07-31' ORDER BY symbol",
        )
        .expect("final_score symbol query");
        let symbols: Vec<&str> = csv.lines().skip(1).collect();
        assert_eq!(
            symbols,
            vec!["SH600001", "SH600002", "SZ000001"],
            "final_score symbols must be prefixed forms: {csv}"
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "technical_factor", "2026-07-31"),
            3
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "capital_factor", "2026-07-31"),
            3
        );
        // The fixture's single "AI概念" membership for SZ000001 joins the
        // prefixed SepaRow (Task 4 prefix-key memberships) → exactly 1 row.
        assert_eq!(
            dolt_count(dolt_tmp.path(), "industry_factor", "2026-07-31"),
            1
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "market_temperature", "2026-07-31"),
            1
        );

        // data_updates carries a row per compute table with the CLI source.
        // data_updates carries a row per compute table with the CLI source.
        let csv = crate::import_dolt::run_dolt_sql_csv(
            dolt_tmp.path(),
            "SELECT table_name, source, last_report_date FROM data_updates ORDER BY table_name",
        )
        .expect("data_updates query");
        for table in COMPUTE_TABLES {
            assert!(csv.contains(table), "data_updates missing {table}: {csv}");
        }
        assert!(csv.contains("compass-data sepa"), "source: {csv}");
        assert!(csv.contains("2026-07-31"), "last_report_date: {csv}");
    }

    #[test]
    fn run_score_is_idempotent_on_same_date() {
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");
        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));

        run_score(50, date, &reader, dolt_tmp.path()).expect("first run");
        run_score(50, date, &reader, dolt_tmp.path()).expect("second run");

        // DELETE + append semantics: rows must not accumulate.
        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"), 3);
        assert_eq!(
            dolt_count(dolt_tmp.path(), "market_temperature", "2026-07-31"),
            1
        );
        let total: i64 = dolt_count(dolt_tmp.path(), "final_score", "2026-07-31");
        assert_eq!(total, 3, "idempotent re-run must not add rows");
    }

    #[test]
    fn run_score_preserves_other_dates() {
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        let date = NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
        run_score(50, Some(date), &reader, dolt_tmp.path()).expect("run 07-31");

        // A second computation date only touches its own trade_date.
        let date2 = NaiveDate::from_ymd_opt(2026, 7, 30).expect("date2");
        run_score(50, Some(date2), &reader, dolt_tmp.path()).expect("run 07-30");

        assert_eq!(
            dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"),
            3,
            "07-31 rows must survive the 07-30 run"
        );
        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-30"), 3);
    }

    #[test]
    fn run_temperature_writes_market_temperature_row() {
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        run_temperature(&reader, dolt_tmp.path()).expect("run_temperature");

        let csv = crate::import_dolt::run_dolt_sql_csv(
            dolt_tmp.path(),
            "SELECT trade_date, score, position_suggestion FROM market_temperature",
        )
        .expect("temperature query");
        let lines: Vec<&str> = csv.lines().skip(1).collect();
        assert_eq!(lines.len(), 1, "exactly one temperature row: {csv}");
        let row = lines[0];
        let fields: Vec<&str> = row.split(',').collect();
        assert!(fields[1].parse::<f64>().is_ok(), "score is numeric: {row}");
        assert!(!fields[2].is_empty(), "position suggestion present: {row}");
    }

    #[test]
    fn run_score_with_smaller_top_preserves_all_stored_rows() {
        // P0-1 regression (epic #139 PR review): `--top` is an output cap
        // only; the Dolt write-back must persist the full computed set.
        // Re-running with a smaller top must never delete previously stored
        // rows for the date.
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");
        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));

        run_score(50, date, &reader, dolt_tmp.path()).expect("full run");
        run_score(1, date, &reader, dolt_tmp.path()).expect("top-1 re-run");

        // All 3 fixture stocks must still be present (not truncated to 1).
        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"), 3);
        assert_eq!(
            dolt_count(dolt_tmp.path(), "technical_factor", "2026-07-31"),
            3
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "capital_factor", "2026-07-31"),
            3
        );
    }

    #[test]
    fn run_temperature_does_not_touch_factor_tables() {
        // P0-2 regression (epic #139 PR review): `sepa temperature` must only
        // write market_temperature; the factor/score tables written by a prior
        // score run must survive untouched.
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");
        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));

        run_score(50, date, &reader, dolt_tmp.path()).expect("score");
        run_temperature(&reader, dolt_tmp.path()).expect("temperature");

        // Score rows for the latest trading day must survive the temperature
        // run, and no score rows may appear for other (wall-clock) dates.
        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"), 3);
        assert_eq!(
            dolt_count(dolt_tmp.path(), "technical_factor", "2026-07-31"),
            3
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "capital_factor", "2026-07-31"),
            3
        );
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(
            dolt_count(dolt_tmp.path(), "final_score", &today),
            0,
            "temperature must not add score rows for the wall-clock date"
        );
        // Temperature row lands on the latest trading day (decision 22).
        assert_eq!(
            dolt_count(dolt_tmp.path(), "market_temperature", "2026-07-31"),
            1
        );
    }

    #[test]
    fn run_score_default_date_is_latest_trading_day() {
        // Decision 22 regression: with no --date, run_score must score the
        // latest trading day present in the data (fixture max 2026-07-31),
        // never the wall-clock date (2026-08-02 is a Sunday).
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        run_score(50, None, &reader, dolt_tmp.path()).expect("score default date");

        assert_eq!(dolt_count(dolt_tmp.path(), "final_score", "2026-07-31"), 3);
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(
            dolt_count(dolt_tmp.path(), "final_score", &today),
            0,
            "default date must not be the wall-clock date"
        );
    }

    #[test]
    fn run_score_returns_err_when_dolt_is_missing() {
        // Valid parquet, but the dolt dir is not a repo → write-back must
        // fail loudly instead of silently skipping.
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        build_fixture(parquet_tmp.path());
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));
        let result = run_score(50, date, &reader, dolt_tmp.path());
        assert!(
            result.is_err(),
            "missing dolt repo must fail the write-back"
        );
    }

    #[test]
    fn run_score_returns_err_when_reader_has_no_data() {
        // Corrupt parquet file → fetch_cross_section fails → Err before any
        // write-back happens.
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
        std::fs::write(
            parquet_tmp.path().join("stock_daily.parquet"),
            b"this is not a parquet file",
        )
        .expect("write garbage parquet");
        let reader = ParquetReader::new(parquet_tmp.path()).expect("reader");

        let date = Some(NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"));
        let result = run_score(50, date, &reader, dolt_tmp.path());
        assert!(
            result.is_err(),
            "unreadable parquet must fail run_score before write-back"
        );
    }

    // -----------------------------------------------------------------------
    // Thermometer value-text parsing (locked formats in temperature.rs)
    // -----------------------------------------------------------------------

    #[test]
    fn thermometer_row_extracts_all_five_components() {
        let tm = MarketThermometer {
            score: 63.4,
            position_key: "sepa.position.mid",
            position_pct: 55.0,
            indicators: vec![
                compass_types::SepaIndicator {
                    label_key: "sepa.indicator.hs300_trend",
                    value: 45.6,
                    unit_key: "sepa.unit.percent",
                    delta_pct: None,
                    heat: 0.5,
                },
                compass_types::SepaIndicator {
                    label_key: "sepa.indicator.zz1000_trend",
                    value: 30.0,
                    unit_key: "sepa.unit.percent",
                    delta_pct: None,
                    heat: 0.3,
                },
                compass_types::SepaIndicator {
                    label_key: "sepa.indicator.limit_up",
                    value: 42.0,
                    unit_key: "sepa.unit.count",
                    delta_pct: None,
                    heat: 0.5,
                },
                compass_types::SepaIndicator {
                    label_key: "sepa.indicator.amount",
                    value: 1.20,
                    unit_key: "sepa.unit.trillion",
                    delta_pct: None,
                    heat: 1.0,
                },
                compass_types::SepaIndicator {
                    label_key: "sepa.indicator.breadth",
                    value: 64.3,
                    unit_key: "sepa.unit.percent",
                    delta_pct: None,
                    heat: 0.6,
                },
            ],
        };
        let row = thermometer_csv_row(&tm, NaiveDate::from_ymd_opt(2026, 7, 31).unwrap());
        assert!(row.contains("63.4"), "score: {row}");
        assert!(row.contains("45.6"), "hs300_trend: {row}");
        assert!(row.contains("30.0"), "zz1000_trend: {row}");
        assert!(row.contains(",42,"), "limit_up_count: {row}");
        assert!(
            row.contains("1.2e12") || row.contains("1200000000000"),
            "total_amount: {row}"
        );
        assert!(row.contains("64.3"), "breadth: {row}");
        assert!(row.contains("40%-70%"), "position: {row}");
    }

    #[test]
    fn dolt_import_returns_error_for_unknown_table() {
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());

        let csv_tmp = tempfile::tempdir().expect("csv tmp");
        let csv_path = csv_tmp.path().join("rows.csv");
        std::fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");

        let result = dolt_import(dolt_tmp.path(), "no_such_table", &csv_path);
        assert!(result.is_err(), "dolt import of a missing table must fail");
    }

    #[test]
    fn write_back_skips_tables_without_data_rows() {
        let _lock = crate::tests::ENV_MUTEX.lock().unwrap();
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());

        let data = SepaData {
            rows: Vec::new(),
            thermometer: MarketThermometer {
                score: 50.0,
                position_key: "sepa.position.mid",
                position_pct: 55.0,
                indicators: vec![
                    compass_types::SepaIndicator {
                        label_key: "sepa.indicator.hs300_trend",
                        value: 50.0,
                        unit_key: "sepa.unit.percent",
                        delta_pct: None,
                        heat: 0.5,
                    },
                    compass_types::SepaIndicator {
                        label_key: "sepa.indicator.zz1000_trend",
                        value: 50.0,
                        unit_key: "sepa.unit.percent",
                        delta_pct: None,
                        heat: 0.5,
                    },
                    compass_types::SepaIndicator {
                        label_key: "sepa.indicator.limit_up",
                        value: 0.0,
                        unit_key: "sepa.unit.count",
                        delta_pct: None,
                        heat: 0.0,
                    },
                    compass_types::SepaIndicator {
                        label_key: "sepa.indicator.amount",
                        value: 0.0,
                        unit_key: "sepa.unit.trillion",
                        delta_pct: None,
                        heat: 0.0,
                    },
                    compass_types::SepaIndicator {
                        label_key: "sepa.indicator.breadth",
                        value: 50.0,
                        unit_key: "sepa.unit.percent",
                        delta_pct: None,
                        heat: 0.5,
                    },
                ],
            },
            date: "2026-07-31".to_string(),
        };

        write_back(
            dolt_tmp.path(),
            &data,
            &["technical_factor", "market_temperature"],
        )
        .expect("write_back with empty rows");

        // technical_factor CSV was header-only → skipped; the thermometer
        // row was still appended to market_temperature.
        assert_eq!(
            dolt_count(dolt_tmp.path(), "technical_factor", "2026-07-31"),
            0
        );
        assert_eq!(
            dolt_count(dolt_tmp.path(), "market_temperature", "2026-07-31"),
            1
        );
    }
}
