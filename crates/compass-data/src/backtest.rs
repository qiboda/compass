//! SEPA backtest CLI + Dolt write-back (issue #154).
//!
//! Runs the backtest engine ([`compass_strategy::sepa::backtest`]) over a
//! historical window and writes the daily equity curve into the
//! `backtest_result` Dolt table via the established two-stage pattern
//! (full-table DELETE + `dolt table import -a`), registering `data_updates`.

use std::error::Error;
use std::path::Path;

use chrono::{NaiveDate, Utc};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::backtest::{BacktestParams, equity_csv, run_backtest};

use crate::sepa::{dolt_import, dolt_sql, dolt_upsert_updates, fmt_double, stage_csv};

/// `backtest_result` DDL: daily equity curve, PK on trade_date (aligned with
/// the `market_temperature` single-date-table convention).
pub(crate) const BACKTEST_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS backtest_result (\
  trade_date DATE NOT NULL, \
  strategy_nav DOUBLE, \
  benchmark_nav DOUBLE, \
  update_date DATE, \
  PRIMARY KEY (trade_date))";

/// Run the backtest CLI: compute, print the summary, write the optional CSV
/// file and write the equity curve back to Dolt.
#[allow(clippy::too_many_arguments)]
pub fn run_backtest_cli(
    top: usize,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    days: usize,
    cost: f64,
    csv: Option<&Path>,
    reader: &ParquetReader,
    dolt_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let params = BacktestParams {
        start: start.unwrap_or_else(|| {
            NaiveDate::parse_from_str(
                compass_strategy::sepa::backtest::DEFAULT_BACKTEST_START,
                "%Y-%m-%d",
            )
            .expect("static default start parses")
        }),
        end,
        top_n: top,
        hold_days: days,
        cost,
    };
    let result = run_backtest(&params, reader)?;

    print_summary(&result);
    if let Some(path) = csv {
        std::fs::write(path, equity_csv(&result.points))?;
        println!("equity curve written to {}", path.display());
    }
    let window_end = result
        .points
        .last()
        .map(|p| p.trade_date)
        .unwrap_or(params.start);
    write_back_result(dolt_dir, &result.points, window_end)?;
    Ok(())
}

/// Print the strategy/benchmark summary metrics table.
fn print_summary(result: &compass_strategy::sepa::backtest::BacktestResult) {
    let m = &result.metrics;
    let first = result
        .points
        .first()
        .map_or_else(|| "-".to_string(), |p| p.trade_date.to_string());
    let last = result
        .points
        .last()
        .map_or_else(|| "-".to_string(), |p| p.trade_date.to_string());
    println!("=== SEPA backtest summary ===");
    println!("window: {first} .. {last}");
    println!("rebalances: {}", m.rebalance_count);
    println!(
        "strategy cumulative return: {:.2}%",
        m.cumulative_return * 100.0
    );
    println!(
        "strategy annualized return: {:.2}%",
        m.annualized_return * 100.0
    );
    println!("win rate: {:.1}%", m.win_rate * 100.0);
    println!("profit/loss ratio: {:.2}", m.profit_loss_ratio);
    println!("max drawdown: {:.2}%", m.max_drawdown * 100.0);
    println!(
        "benchmark cumulative return: {:.2}%",
        m.benchmark_cumulative_return * 100.0
    );
    println!("excess return: {:.2}%", m.excess_return * 100.0);
    println!("annualized excess: {:.2}%", m.annualized_excess * 100.0);
}

/// Two-stage write-back of the equity curve (issue #154 decision 7): full
/// DELETE then append CSV via `dolt table import -a`. `backtest_result` is a
/// single-run snapshot table (PK trade_date) — each run replaces the whole
/// curve so windows from earlier runs with a different `--start` cannot
/// accumulate stale rows.
fn write_back_result(
    dolt_dir: &Path,
    points: &[compass_strategy::sepa::backtest::EquityPoint],
    end: NaiveDate,
) -> Result<(), Box<dyn Error>> {
    let today = Utc::now().date_naive();

    dolt_sql(dolt_dir, BACKTEST_SCHEMA)?;
    dolt_sql(dolt_dir, crate::sepa::UPDATES_SCHEMA)?;
    dolt_sql(dolt_dir, "DELETE FROM backtest_result")?;

    if points.is_empty() {
        return Ok(());
    }

    let mut csv = String::from("trade_date,strategy_nav,benchmark_nav,update_date\n");
    for p in points {
        csv.push_str(&format!(
            "{},{},{},{}\n",
            p.trade_date.format("%Y-%m-%d"),
            fmt_double(p.strategy_nav),
            fmt_double(p.benchmark_nav),
            today
        ));
    }

    let path = stage_csv(&format!("{end}_backtest_result"), &csv)?;
    let import = dolt_import(dolt_dir, "backtest_result", &path);
    let _ = std::fs::remove_file(&path);
    import?;
    let row_count = csv.lines().count() - 1;
    dolt_upsert_updates(dolt_dir, "backtest_result", today, end, row_count)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use chrono::NaiveDate;
    use compass_strategy::sepa::backtest::{EquityPoint, equity_csv};

    /// Serialise Dolt tests: dolt reads the process-global HOME, racing with
    /// main.rs's HOME-mutating tests (sepa.rs test convention).
    fn dolt_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::tests::ENV_MUTEX.lock().unwrap()
    }

    fn setup_dolt(dir: &Path) {
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

    fn table_count(dolt_dir: &Path, table: &str) -> i64 {
        let csv = crate::import_dolt::run_dolt_sql_csv(
            dolt_dir,
            &format!("SELECT COUNT(*) AS cnt FROM {table}"),
        )
        .expect("count query");
        csv.lines()
            .nth(1)
            .and_then(|l| l.parse::<i64>().ok())
            .expect("parse count")
    }

    fn upsert_row(dolt_dir: &Path, table: &str) -> String {
        crate::import_dolt::run_dolt_sql_csv(
            dolt_dir,
            &format!("SELECT last_report_date AS d FROM data_updates WHERE table_name = '{table}'"),
        )
        .expect("upsert query")
    }

    fn points_fixture() -> Vec<EquityPoint> {
        vec![
            EquityPoint {
                trade_date: NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
                strategy_nav: 1.0,
                benchmark_nav: 1.0,
            },
            EquityPoint {
                trade_date: NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap(),
                strategy_nav: 1.05,
                benchmark_nav: 1.02,
            },
        ]
    }

    /// Full-table count after a write equals points.len(); rerun is
    /// idempotent; data_updates registered with last_report_date = end.
    #[test]
    fn write_back_result_roundtrip() {
        let _lock = dolt_guard();
        let dir = tempfile::tempdir().expect("tempdir");
        setup_dolt(dir.path());

        let pts = points_fixture();
        let end = NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap();
        write_back_result(dir.path(), &pts, end).expect("write");

        assert_eq!(table_count(dir.path(), "backtest_result"), 2);
        // Idempotent rerun: range DELETE + import keeps the count stable.
        write_back_result(dir.path(), &pts, end).expect("rewrite");
        assert_eq!(table_count(dir.path(), "backtest_result"), 2);

        let upsert = upsert_row(dir.path(), "backtest_result");
        assert!(
            upsert.contains("2025-01-03"),
            "last_report_date should be end date, got: {upsert}"
        );

        // Values readable back from Dolt match the CSV input.
        let csv = crate::import_dolt::run_dolt_sql_csv(
            dir.path(),
            "SELECT strategy_nav, benchmark_nav FROM backtest_result ORDER BY trade_date",
        )
        .expect("readback");
        let rows: Vec<&str> = csv.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(rows.len(), 3, "header + 2 data rows, got: {csv}");
        assert_eq!(rows[1], "1,1", "row1 (dolt CSV format): {}", rows[1]);
        assert_eq!(rows[2], "1.05,1.02", "row2: {}", rows[2]);
    }

    /// Empty points: no import, no panic; data_updates untouched.
    #[test]
    fn write_back_result_empty() {
        let _lock = dolt_guard();
        let dir = tempfile::tempdir().expect("tempdir");
        setup_dolt(dir.path());
        let end = NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap();
        write_back_result(dir.path(), &[], end).expect("empty ok");
        assert_eq!(table_count(dir.path(), "backtest_result"), 0);
    }

    /// Regression (ref #184): two write-back runs sharing the same `end`
    /// must not race on a shared temp CSV path, and must clean up their
    /// staged files. The old fixed path `{end}_backtest_result.csv` raced
    /// under nextest parallelism — a second run could overwrite the CSV
    /// while the first was importing it.
    #[test]
    fn write_back_result_stages_and_cleans_temp_file() {
        let _lock = dolt_guard();
        let dir1 = tempfile::tempdir().expect("tempdir1");
        let dir2 = tempfile::tempdir().expect("tempdir2");
        setup_dolt(dir1.path());
        setup_dolt(dir2.path());

        // Given stale files from earlier runs of this test exist, remove
        // them so the post-condition below counts only this test's runs.
        // Seed one first so the cleanup loop's remove-file path executes.
        let temp_dir = std::env::temp_dir().join("compass_sepa_writeback");
        std::fs::create_dir_all(&temp_dir).expect("create writeback dir");
        std::fs::write(
            temp_dir.join("2025-06-30_backtest_result_stale.csv"),
            "stale",
        )
        .expect("seed stale file");
        if let Ok(rd) = std::fs::read_dir(&temp_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("2025-06-30_backtest_result") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }

        let pts = points_fixture();
        let end = NaiveDate::parse_from_str("2025-06-30", "%Y-%m-%d").unwrap();
        write_back_result(dir1.path(), &pts, end).expect("first write");
        write_back_result(dir2.path(), &pts, end).expect("second write");

        // When two runs share an end date, both must import their data and
        // leave no staged temp CSV behind (unique names + post-import
        // cleanup; the old shared path left one file and raced).
        let leftovers = std::fs::read_dir(&temp_dir)
            .expect("read temp dir")
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("2025-06-30_backtest_result")
            })
            .count();
        assert_eq!(
            leftovers, 0,
            "staged temp CSVs must be removed after import, got {leftovers} leftover(s)"
        );
        assert_eq!(table_count(dir1.path(), "backtest_result"), 2);
        assert_eq!(table_count(dir2.path(), "backtest_result"), 2);
    }

    /// Full-table replace: a second write with a narrower curve leaves only
    /// the new rows (earlier windows do not accumulate).
    #[test]
    fn write_back_result_full_replace() {
        let _lock = dolt_guard();
        let dir = tempfile::tempdir().expect("tempdir");
        setup_dolt(dir.path());

        let pts = points_fixture();
        let end = NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap();

        // Second write with a narrower curve (only the first point): the
        // second day's row must be gone — the table holds only the new run.
        let narrow = vec![pts[0].clone()];
        write_back_result(dir.path(), &narrow, end).expect("narrow write");
        assert_eq!(table_count(dir.path(), "backtest_result"), 1);
    }

    /// CLI entry composes end-to-end when given a parquet reader: the CSV
    /// file is written with header + rows and Dolt receives the curve.
    /// (run_backtest needs a real parquet fixture; mirror sepa.rs minimal
    /// fixture shape with high enough volume to pass the liquidity filter.)
    #[test]
    fn run_backtest_cli_end_to_end() {
        use compass_core::data::parquet::ParquetReader;
        use duckdb::Connection;

        let _lock = dolt_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let dolt_dir = tempfile::tempdir().expect("dolt tempdir");
        setup_dolt(dolt_dir.path());

        let conn = Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        let dates = [
            "2024-12-30",
            "2024-12-31",
            "2025-01-02",
            "2025-01-03",
            "2025-01-06",
            "2025-01-07",
            "2025-01-08",
            "2025-01-09",
            "2025-01-10",
            "2025-01-13",
            "2025-01-14",
        ];
        for (i, d) in dates.iter().enumerate() {
            for sym in ["600001", "600002", "600003"] {
                let close = 10.0 * (1.0 + 0.01 * i as f64);
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        sym,
                        d,
                        close - 0.01,
                        close,
                        close - 0.02,
                        close,
                        close,
                        5e7,
                        5e8
                    ],
                )
                .expect("insert daily");
            }
        }
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy daily");

        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .expect("create basic");
        for sym in ["600001", "600002", "600003"] {
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, '2024-01-01', NULL, '主板', ?, 1e9, '测试', NULL)",
                duckdb::params![sym, sym, "SH", sym],
            )
            .expect("insert basic");
        }
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_basic.parquet").display()
        ))
        .expect("copy basic");

        let reader = ParquetReader::new(tmp.path()).expect("reader");
        let csv_path = tmp.path().join("curve.csv");
        let start = NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap();
        let end = Some(NaiveDate::parse_from_str("2025-01-14", "%Y-%m-%d").unwrap());

        run_backtest_cli(
            2,
            Some(start),
            end,
            5,
            0.0,
            Some(&csv_path),
            &reader,
            dolt_dir.path(),
        )
        .expect("cli runs");

        let csv = std::fs::read_to_string(&csv_path).expect("csv exists");
        let lines: Vec<&str> = csv.trim().split('\n').collect();
        assert_eq!(lines[0], "trade_date,strategy_nav,benchmark_nav");
        assert_eq!(lines.len(), 10, "header + 9 rows, got: {}", lines.len());

        let count = table_count(dolt_dir.path(), "backtest_result");
        assert_eq!(count, 9, "Dolt should hold the 9 output days");
    }

    /// `start=None` falls back to DEFAULT_BACKTEST_START (the unwrap_or_else
    /// closure), and a csv path writes the equity curve file.
    #[test]
    fn run_backtest_cli_default_start_writes_csv() {
        use compass_core::data::parquet::ParquetReader;
        use duckdb::Connection;

        let _lock = dolt_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let dolt_dir = tempfile::tempdir().expect("dolt tempdir");
        setup_dolt(dolt_dir.path());

        let conn = Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        let dates = [
            "2024-12-30",
            "2024-12-31",
            "2025-01-02",
            "2025-01-03",
            "2025-01-06",
            "2025-01-07",
            "2025-01-08",
            "2025-01-09",
            "2025-01-10",
            "2025-01-13",
            "2025-01-14",
        ];
        for (i, d) in dates.iter().enumerate() {
            for sym in ["600001", "600002", "600003"] {
                let close = 10.0 * (1.0 + 0.01 * i as f64);
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        sym,
                        d,
                        close - 0.01,
                        close,
                        close - 0.02,
                        close,
                        close,
                        5e7,
                        5e8
                    ],
                )
                .expect("insert daily");
            }
        }
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy daily");

        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .expect("create basic");
        for sym in ["600001", "600002", "600003"] {
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, '2024-01-01', NULL, '主板', ?, 1e9, '测试', NULL)",
                duckdb::params![sym, sym, "SH", sym],
            )
            .expect("insert basic");
        }
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_basic.parquet").display()
        ))
        .expect("copy basic");

        let reader = ParquetReader::new(tmp.path()).expect("reader");
        let csv_path = tmp.path().join("curve.csv");
        let end = Some(NaiveDate::parse_from_str("2025-01-14", "%Y-%m-%d").unwrap());

        run_backtest_cli(
            2,
            None,
            end,
            5,
            0.0,
            Some(&csv_path),
            &reader,
            dolt_dir.path(),
        )
        .expect("cli runs");

        let csv = std::fs::read_to_string(&csv_path).expect("csv exists");
        assert!(csv.starts_with("trade_date,strategy_nav,benchmark_nav\n"));
        let count = table_count(dolt_dir.path(), "backtest_result");
        assert!(count > 0, "Dolt should hold output days");
    }

    /// CSV file written by the CLI matches equity_csv output.
    #[test]
    fn cli_csv_matches_equity_csv() {
        let pts = points_fixture();
        let csv = equity_csv(&pts);
        assert!(csv.starts_with("trade_date,strategy_nav,benchmark_nav\n"));
        assert!(csv.contains("2025-01-02,1.0,1.0"));
        assert!(csv.contains("2025-01-03,1.050000,1.020000"));
    }
}
