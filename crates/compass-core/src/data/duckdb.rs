#![allow(missing_docs)]
// Record types below are data bags with self-explanatory field names.
// Requiring doc comments on every field would reduce readability.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use duckdb::{Connection, OptionalExt, params};
use egui_charts::model::Bar;
use tracing;

use crate::data::provider::{DataError, DataProvider, DataWriter, NegativeCache};
use crate::indicators::{AdjustMode, RawBar, adjust_ohlc};
use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// Type-safe record structs for all 7 tables
// ---------------------------------------------------------------------------

/// A single row from the `stock_daily` table — one trading day's OHLCV data.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyRecord {
    pub trade_date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub adjclose: f64,
    pub volume: f64,
    pub amount: f64,
}

/// A single row from the `stock_adj_factor` table — per-day price adjustment multiplier.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjFactorRecord {
    pub trade_date: NaiveDate,
    pub adj_factor: f64,
}

/// A single row from the trade status table — whether the market was open.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRecord {
    pub trade_date: NaiveDate,
    pub is_open: bool,
}

/// A single row from the `stock_limit` table — daily price ceiling and floor.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitRecord {
    pub trade_date: NaiveDate,
    pub up_limit: f64,
    pub down_limit: f64,
}

/// A single row from the indicator table — turnover rate, P/E, P/B, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorRecord {
    pub trade_date: NaiveDate,
    pub turnover_rate: f64,
    pub turnover_rate_f: f64,
    pub volume_ratio: f64,
    pub pe: f64,
    pub pe_ttm: f64,
    pub pb: f64,
    pub ps: f64,
}

/// A single row from the share table — share count and market value.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareRecord {
    pub trade_date: NaiveDate,
    pub total_share: f64,
    pub float_share: f64,
    pub free_share: f64,
    pub total_mv: f64,
    pub circ_mv: f64,
}

// ---------------------------------------------------------------------------
// Schema DDL — 7 tables + indexes + no_data_marks
// ---------------------------------------------------------------------------

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS stock_daily (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open        DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    close       DOUBLE,
    adjclose    DOUBLE,
    volume      DOUBLE,
    amount      DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_adj_factor (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    adj_factor  DOUBLE NOT NULL,
    PRIMARY KEY (symbol, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_limit (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    up_limit    DOUBLE,
    down_limit  DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_date ON stock_daily(trade_date);
CREATE INDEX IF NOT EXISTS idx_adj_date ON stock_adj_factor(trade_date);
CREATE INDEX IF NOT EXISTS idx_limit_date ON stock_limit(trade_date);

CREATE TABLE IF NOT EXISTS no_data_marks (
    symbol       TEXT NOT NULL,
    timeframe    TEXT NOT NULL,
    last_checked BIGINT NOT NULL,
    PRIMARY KEY (symbol, timeframe)
);
";

// ---------------------------------------------------------------------------
// DuckDbProvider — in-memory cache with optional Parquet backing
// ---------------------------------------------------------------------------

/// In-memory DuckDB cache with optional Parquet backing (ref #31).
///
/// Implements all three provider traits (`DataProvider`, `DataWriter`,
/// `NegativeCache`) on a single in-memory DuckDB connection wrapped in
/// `Arc<Mutex<>>`. When `parquet_dir` is set, `fetch_bars` reads from
/// a single `stock_daily.parquet` file on cache miss.
///
/// Use `new(Some(parquet_dir))` for production or `new_in_memory()` for tests.
pub struct DuckDbProvider {
    pub(crate) conn: Arc<Mutex<Connection>>,
    parquet_dir: Option<std::path::PathBuf>,
}

impl DuckDbProvider {
    /// Open an in-memory DuckDB database with optional Parquet backing (ref #31).
    ///
    /// When `parquet_dir` is set, `fetch_bars` falls back to reading from
    /// `{parquet_dir}/stock_daily.parquet` on cache miss.
    pub fn new(parquet_dir: Option<std::path::PathBuf>) -> Result<Self, DataError> {
        let conn = Connection::open_in_memory().map_err(DataError::Database)?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(DataError::Database)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            parquet_dir,
        })
    }

    /// Open (or create) a file-backed DuckDB at `path` and ensure the schema exists.
    ///
    /// Used by CLI tools (export, download) that need persistent storage.
    pub fn new_file(path: &str) -> Result<Self, DataError> {
        let conn = Connection::open(path).map_err(DataError::Database)?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(DataError::Database)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            parquet_dir: None,
        })
    }

    /// Convenience constructor for tests — opens an in-memory database.
    pub fn new_in_memory() -> Result<Self, DataError> {
        Self::new(None)
    }

    /// Access the underlying DuckDB connection for direct queries (tests only).
    pub fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DataError> {
        self.conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))
    }

    /// Execute an arbitrary SQL batch (for maintenance/export operations).
    ///
    /// SQL statements are separated by `;`.  Only use this for trusted,
    /// static SQL — never concatenate user input.
    pub async fn execute_batch(&self, sql: &str) -> Result<(), DataError> {
        let sql = sql.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            conn.execute_batch(&sql).map_err(DataError::Database)
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Check whether a table has any rows by executing a COUNT query.
    pub async fn table_has_rows(&self, count_sql: &str) -> Result<bool, DataError> {
        let count_sql = count_sql.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let count: i64 = conn
                .query_row(&count_sql, [], |row| row.get(0))
                .map_err(DataError::Database)?;
            Ok(count > 0)
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    // -----------------------------------------------------------------------
    // Helper: convert ts_code + date → DateTime<Utc> for Bar.time
    // -----------------------------------------------------------------------

    fn date_str_to_utc(date_str: &str) -> Option<DateTime<Utc>> {
        if let Ok(naive) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            let naive_dt = naive.and_hms_opt(0, 0, 0)?;
            return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
        }
        if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
            return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
        }
        if let Ok(naive_dt) =
            chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S%.f")
        {
            return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
        }
        None
    }

    // -----------------------------------------------------------------------
    // Direct table methods (non-trait) — for use by CLI downloader
    // -----------------------------------------------------------------------

    /// Return the MIN and MAX `trade_date` for a given `symbol`, or `None` if
    /// no data exists.
    pub async fn get_stored_range(
        &self,
        symbol: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        let symbol = symbol.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "SELECT CAST(MIN(trade_date) AS VARCHAR), CAST(MAX(trade_date) AS VARCHAR) FROM stock_daily WHERE symbol = ?",
                )
                .map_err(DataError::Database)?;

            let result: Option<(Option<String>, Option<String>)> = stmt
                .query_row(params![symbol], |row| {
                    Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .optional()
                .map_err(DataError::Database)?;

            match result {
                Some((Some(min_s), Some(max_s))) if !min_s.is_empty() && !max_s.is_empty() => {
                    let min_date =
                        NaiveDate::parse_from_str(&min_s, "%Y-%m-%d").map_err(|e| {
                            DataError::Parse(format!("invalid min date '{min_s}': {e}"))
                        })?;
                    let max_date =
                        NaiveDate::parse_from_str(&max_s, "%Y-%m-%d").map_err(|e| {
                            DataError::Parse(format!("invalid max date '{max_s}': {e}"))
                        })?;
                    Ok(Some((min_date, max_date)))
                }
                _ => Ok(None),
            }
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save a batch of daily OHLCV records into `stock_daily`.
    ///
    /// Records are sorted by `trade_date` in ascending order.  `pre_close` is
    /// computed from the preceding record's `close` (`NULL` for the first
    /// record in the batch).
    ///
    /// When `overwrite` is false, existing (symbol, trade_date) rows are skipped
    /// (migration-style).  When true, existing rows are replaced.
    pub async fn save_stock_daily(
        &self,
        symbol: &str,
        records: &[DailyRecord],
        overwrite: bool,
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let symbol = symbol.to_string();
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = records
            .iter()
            .map(|r| {
                (
                    r.trade_date.format("%Y-%m-%d").to_string(),
                    r.open,
                    r.high,
                    r.low,
                    r.close,
                    r.adjclose,
                    r.volume,
                    r.amount,
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let verb = if overwrite {
                "INSERT OR REPLACE"
            } else {
                "INSERT OR IGNORE"
            };
            let sql = format!(
                "{verb} INTO stock_daily
                    (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            );
            let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;

            for (date_str, open, high, low, close, adjclose, volume, amount) in &rows {
                stmt.execute(params![
                    symbol, date_str, open, high, low, close, adjclose, volume, amount,
                ])
                .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save adj_factor records into `stock_adj_factor`.
    ///
    /// When `overwrite` is false, existing (symbol, trade_date) rows are skipped.
    /// When true, existing rows are replaced.
    pub async fn save_adj_factors(
        &self,
        symbol: &str,
        factors: &[AdjFactorRecord],
        overwrite: bool,
    ) -> Result<(), DataError> {
        if factors.is_empty() {
            return Ok(());
        }

        let symbol = symbol.to_string();
        let owned: Vec<(String, f64)> = factors
            .iter()
            .map(|r| (r.trade_date.format("%Y-%m-%d").to_string(), r.adj_factor))
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let verb = if overwrite {
                "INSERT OR REPLACE"
            } else {
                "INSERT OR IGNORE"
            };
            let sql = format!(
                "{verb} INTO stock_adj_factor (symbol, trade_date, adj_factor) VALUES (?, ?, ?)"
            );
            let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;

            for (date_str, factor) in &owned {
                stmt.execute(params![symbol, date_str, factor])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Return the MIN and MAX trade_date for adj_factor of a symbol.
    pub async fn get_adj_factor_range(
        &self,
        symbol: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        let symbol = symbol.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "SELECT CAST(MIN(trade_date) AS VARCHAR), CAST(MAX(trade_date) AS VARCHAR) FROM stock_adj_factor WHERE symbol = ?",
                )
                .map_err(DataError::Database)?;

            let result: Option<(Option<String>, Option<String>)> = stmt
                .query_row(params![symbol], |row| {
                    Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .optional()
                .map_err(DataError::Database)?;

            match result {
                Some((Some(min_s), Some(max_s))) if !min_s.is_empty() && !max_s.is_empty() => {
                    let min_date =
                        NaiveDate::parse_from_str(&min_s, "%Y-%m-%d").map_err(|e| {
                            DataError::Parse(format!("invalid min date '{min_s}': {e}"))
                        })?;
                    let max_date =
                        NaiveDate::parse_from_str(&max_s, "%Y-%m-%d").map_err(|e| {
                            DataError::Parse(format!("invalid max date '{max_s}': {e}"))
                        })?;
                    Ok(Some((min_date, max_date)))
                }
                _ => Ok(None),
            }
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save limit records into `stock_limit`.
    ///
    /// When `overwrite` is false, existing (symbol, trade_date) rows are skipped.
    /// When true, existing rows are replaced.
    pub async fn save_limits(
        &self,
        symbol: &str,
        records: &[LimitRecord],
        overwrite: bool,
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let symbol = symbol.to_string();
        let owned: Vec<(String, f64, f64)> = records
            .iter()
            .map(|r| {
                (
                    r.trade_date.format("%Y-%m-%d").to_string(),
                    r.up_limit,
                    r.down_limit,
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let verb = if overwrite {
                "INSERT OR REPLACE"
            } else {
                "INSERT OR IGNORE"
            };
            let sql = format!(
                "{verb} INTO stock_limit (symbol, trade_date, up_limit, down_limit) VALUES (?, ?, ?, ?)"
            );
            let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;

            for (date_str, up_limit, down_limit) in &owned {
                stmt.execute(params![symbol, date_str, up_limit, down_limit])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }
}

// ---------------------------------------------------------------------------
// DataProvider — read-only data source
// ---------------------------------------------------------------------------

/// One row from the daily queries: (date_str, open, high, low, close, volume,
/// adjclose). `adjclose` is optional — rows written without it (e.g. via
/// `save_bars`) fall back to factor 1.0 during forward adjustment.
type DailyRow = (String, f64, f64, f64, f64, f64, Option<f64>);

#[async_trait]
impl DataProvider for DuckDbProvider {
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
        adjust: &str,
    ) -> Result<Vec<Bar>, DataError> {
        let symbol = symbol.to_string();
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();
        let conn = Arc::clone(&self.conn);
        let parquet_dir = self.parquet_dir.clone();
        let timeframe = timeframe.to_string();
        let adjust = adjust.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "SELECT CAST(trade_date AS VARCHAR), open, high, low, close, volume, adjclose
                     FROM stock_daily
                     WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
                     ORDER BY trade_date ASC",
                )
                .map_err(DataError::Database)?;

            let mut rows: Vec<DailyRow> = stmt
                .query_map(
                    params![symbol.as_str(), start_str.as_str(), end_str.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                        ))
                    },
                )
                .map_err(DataError::Database)?
                .collect::<Result<Vec<_>, duckdb::Error>>()
                .map_err(DataError::Database)?;

            // Issue #31: fallback to parquet file on cache miss
            if rows.is_empty()
                && let Some(ref parquet_dir) = parquet_dir
            {
                let parquet_path = parquet_dir.join("stock_daily.parquet");
                if parquet_path.exists() {
                    tracing::debug!(
                        symbol = %symbol,
                        parquet = %"stock_daily.parquet",
                        "parquet fallback - reading from single file"
                    );
                    let path_str = parquet_path.to_string_lossy();
                    let sql = format!(
                        "SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume, adjclose, amount
                         FROM read_parquet('{path_str}')
                         WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
                         ORDER BY tradedate ASC"
                    );
                    let mut pstmt = conn.prepare(&sql).map_err(DataError::Database)?;
                    let parquet_rows: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = pstmt
                        .query_map(
                            params![symbol.as_str(), start_str.as_str(), end_str.as_str()],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, f64>(1)?,
                                    row.get::<_, f64>(2)?,
                                    row.get::<_, f64>(3)?,
                                    row.get::<_, f64>(4)?,
                                    row.get::<_, f64>(5)?,
                                    row.get::<_, f64>(6)?,
                                    row.get::<_, f64>(7)?,
                                ))
                            },
                        )
                        .map_err(DataError::Database)?
                        .collect::<Result<Vec<_>, duckdb::Error>>()
                        .map_err(DataError::Database)?;

                    // Carry adjclose through so the daily path below can
                    // forward-adjust; amount is not needed for chart bars.
                    rows = parquet_rows
                        .iter()
                        .map(|(d, o, h, l, c, v, a, _)| {
                            (d.clone(), *o, *h, *l, *c, *v, Some(*a))
                        })
                        .collect();

                    tracing::debug!(
                        symbol = %symbol,
                        rows_from_parquet = rows.len(),
                        "parquet fallback result"
                    );

                    // Cache-warm: persist parquet data into in-memory table
                    if !parquet_rows.is_empty() {
                        let mut insert = conn
                            .prepare(
                                "INSERT OR IGNORE INTO stock_daily
                                 (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                            )
                            .map_err(DataError::Database)?;
                        for (date_str, open, high, low, close, volume, adjclose, amount) in &parquet_rows {
                            insert
                                .execute(params![
                                    symbol.as_str(),
                                    date_str.as_str(),
                                    open,
                                    high,
                                    low,
                                    close,
                                    adjclose,
                                    volume,
                                    amount,
                                ])
                                .map_err(DataError::Database)?;
                        }
                    }
                }
            }

            // Epic #255: dual-parquet fallback — when the stock file yields
            // nothing, route to `index_daily.parquet` (official indexes /
            // BK boards). The two files are mutually exclusive: ref #201
            // removed the 6 indexes from stock_daily, so an empty stock
            // result makes the index lookup deterministic; stock symbols
            // never exist in the index file, so the fallback cannot leak.
            if rows.is_empty()
                && let Some(ref parquet_dir) = parquet_dir
            {
                let index_path = parquet_dir.join("index_daily.parquet");
                if index_path.exists() {
                    tracing::debug!(
                        symbol = %symbol,
                        parquet = %"index_daily.parquet",
                        "index fallback - reading from index file"
                    );
                    let path_str = index_path.to_string_lossy();
                    let sql = format!(
                        "SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume, adjclose, amount
                         FROM read_parquet('{path_str}')
                         WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
                         ORDER BY tradedate ASC"
                    );
                    let mut pstmt = conn.prepare(&sql).map_err(DataError::Database)?;
                    let index_rows: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = pstmt
                        .query_map(
                            params![symbol.as_str(), start_str.as_str(), end_str.as_str()],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, f64>(1)?,
                                    row.get::<_, f64>(2)?,
                                    row.get::<_, f64>(3)?,
                                    row.get::<_, f64>(4)?,
                                    row.get::<_, f64>(5)?,
                                    row.get::<_, f64>(6)?,
                                    row.get::<_, f64>(7)?,
                                ))
                            },
                        )
                        .map_err(DataError::Database)?
                        .collect::<Result<Vec<_>, duckdb::Error>>()
                        .map_err(DataError::Database)?;

                    rows = index_rows
                        .iter()
                        .map(|(d, o, h, l, c, v, a, _)| {
                            (d.clone(), *o, *h, *l, *c, *v, Some(*a))
                        })
                        .collect();

                    tracing::debug!(
                        symbol = %symbol,
                        rows_from_index_parquet = rows.len(),
                        "index fallback result"
                    );

                    // Cache-warm into the in-memory table so the 1w/1M
                    // date_trunc aggregation below (which re-queries
                    // `stock_daily` in memory) covers the index path with
                    // zero extra aggregation logic.
                    if !index_rows.is_empty() {
                        let mut insert = conn
                            .prepare(
                                "INSERT OR IGNORE INTO stock_daily
                                 (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                            )
                            .map_err(DataError::Database)?;
                        for (date_str, open, high, low, close, volume, adjclose, amount) in &index_rows {
                            insert
                                .execute(params![
                                    symbol.as_str(),
                                    date_str.as_str(),
                                    open,
                                    high,
                                    low,
                                    close,
                                    adjclose,
                                    volume,
                                    amount,
                                ])
                                .map_err(DataError::Database)?;
                        }
                    }
                }
            }

            // Issue #46: timeframe aggregation — re-query with date_trunc
            // GROUP BY for weekly/monthly OHLCV resample from daily data.
            // Adjustment (ref #345): daily bars are scaled by the mode's
            // factor BEFORE aggregation, so the weekly MAX(high)/MIN(low) are
            // extremes of the adjusted series. Forward mode normalizes against
            // the last valid ratio of the *same query window* (anchor CTE);
            // invalid rows keep factor 1.0 and are never divided by the anchor.
            if timeframe != "1d" && !rows.is_empty() {
                let unit = match timeframe.as_str() {
                    "1w" => "week",
                    "1M" => "month",
                    _ => "day",
                };
                let mode: AdjustMode = std::str::FromStr::from_str(&adjust).unwrap();
                let (anchor_prefix, scale_expr, anchor_params): (String, String, usize) =
                    match mode {
                        AdjustMode::None => (
                            String::new(),
                            "1.0".to_string(),
                            0,
                        ),
                        AdjustMode::Backward => (
                            String::new(),
                            "CASE WHEN close > 0 AND adjclose IS NOT NULL
                                       AND isfinite(adjclose) AND adjclose > 0
                                  THEN adjclose / close ELSE 1.0 END"
                                .to_string(),
                            0,
                        ),
                        AdjustMode::Forward => (
                            "WITH anchor AS (
                                 SELECT (adjclose / close) AS r
                                 FROM stock_daily
                                 WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
                                   AND close > 0 AND adjclose IS NOT NULL
                                   AND isfinite(adjclose) AND adjclose > 0
                                 ORDER BY trade_date DESC LIMIT 1
                             )"
                            .to_string(),
                            "CASE WHEN close > 0 AND adjclose IS NOT NULL
                                       AND isfinite(adjclose) AND adjclose > 0
                                  THEN (adjclose / close) / (SELECT r FROM anchor)
                                  ELSE 1.0 END"
                                .to_string(),
                            3,
                        ),
                    };
                let sql = format!(
                    "{anchor_prefix}
                     SELECT CAST(grp_date AS VARCHAR) as trade_date,
                            open, high, low, close, volume
                     FROM (
                         SELECT
                             DATE_TRUNC('{unit}', trade_date) as grp_date,
                             FIRST(open) as open,
                             MAX(high) as high,
                             MIN(low) as low,
                             LAST(close) as close,
                             SUM(volume) as volume
                         FROM (
                             SELECT trade_date,
                                    open * scale as open,
                                    high * scale as high,
                                    low * scale as low,
                                    close * scale as close,
                                    volume
                             FROM (
                                 SELECT trade_date, open, high, low, close, volume,
                                        {scale_expr} AS scale
                                 FROM stock_daily
                                 WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
                                 ORDER BY trade_date ASC
                             )
                         )
                         GROUP BY grp_date
                     ) ORDER BY trade_date"
                );
                let mut agg_stmt = conn.prepare(&sql).map_err(DataError::Database)?;
                let bind_vals: Vec<&str> = if anchor_params == 3 {
                    vec![
                        symbol.as_str(),
                        start_str.as_str(),
                        end_str.as_str(),
                        symbol.as_str(),
                        start_str.as_str(),
                        end_str.as_str(),
                    ]
                } else {
                    vec![symbol.as_str(), start_str.as_str(), end_str.as_str()]
                };
                let agg_rows: Vec<(String, f64, f64, f64, f64, f64)> = agg_stmt
                    .query_map(duckdb::params_from_iter(bind_vals), |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                            ))
                        },
                    )
                    .map_err(DataError::Database)?
                    .collect::<Result<Vec<_>, duckdb::Error>>()
                    .map_err(DataError::Database)?;

                let bars: Vec<Bar> = agg_rows
                    .into_iter()
                    .filter_map(|(date_str, open, high, low, close, volume)| {
                        let time = DuckDbProvider::date_str_to_utc(&date_str)?;
                        Some(Bar {
                            time,
                            open,
                            high,
                            low,
                            close,
                            volume,
                        })
                    })
                    .collect();

                return Ok(bars);
            }

            // Daily path: adjust each bar per mode (ref #345). Rows with
            // NULL/invalid adjclose keep factor 1.0 — and never capture the
            // forward anchor (the anchor is the last *valid* ratio).
            let mut raw: Vec<RawBar> = Vec::with_capacity(rows.len());
            let mut adjclose: Vec<Option<f64>> = Vec::with_capacity(rows.len());
            for (date_str, open, high, low, close, volume, adj) in rows {
                let Some(time) = DuckDbProvider::date_str_to_utc(&date_str) else {
                    continue;
                };
                raw.push(RawBar {
                    date: time.date_naive(),
                    open,
                    high,
                    low,
                    close,
                    volume,
                });
                adjclose.push(adj);
            }

            Ok(adjust_ohlc(
                &raw,
                &adjclose,
                adjust.parse().expect("infallible AdjustMode parse"),
            ))
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    async fn search_symbols(&self, _query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        // DuckDB provider does not store symbol metadata.
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// DataWriter — write-through cache interface
// ---------------------------------------------------------------------------

#[async_trait]
impl DataWriter for DuckDbProvider {
    async fn save_bars(
        &self,
        symbol: &str,
        _timeframe: &str,
        bars: &[Bar],
        overwrite: bool,
    ) -> Result<(), DataError> {
        if bars.is_empty() {
            return Ok(());
        }

        let symbol = symbol.to_string();
        let records: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = bars
            .iter()
            .map(|b| {
                (
                    b.time.format("%Y-%m-%d").to_string(),
                    b.open,
                    b.high,
                    b.low,
                    b.close,
                    b.close, // adjclose = close (unadjusted)
                    b.volume,
                    0.0, // amount not available from Bar
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let verb = if overwrite {
                "INSERT OR REPLACE"
            } else {
                "INSERT OR IGNORE"
            };
            let sql = format!(
                "{verb} INTO stock_daily
                    (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            );
            let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;

            for (date_str, open, high, low, close, adjclose, volume, amount) in &records {
                stmt.execute(params![
                    symbol, date_str, open, high, low, close, adjclose, volume, amount
                ])
                .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }
}

// ---------------------------------------------------------------------------
// NegativeCache — mark/fetch negative cache entries
// ---------------------------------------------------------------------------

#[async_trait]
impl NegativeCache for DuckDbProvider {
    async fn mark_no_data(&self, symbol: &str, timeframe: &str) -> Result<(), DataError> {
        let symbol = symbol.to_string();
        let timeframe = timeframe.to_string();
        let now = Utc::now().timestamp();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            conn.execute(
                "INSERT OR REPLACE INTO no_data_marks (symbol, timeframe, last_checked) VALUES (?, ?, ?)",
                params![symbol, timeframe, now],
            )
            .map_err(DataError::Database)?;
            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    async fn is_no_data(
        &self,
        symbol: &str,
        timeframe: &str,
        now_ts: i64,
        ttl_secs: i64,
    ) -> Result<bool, DataError> {
        let symbol = symbol.to_string();
        let timeframe = timeframe.to_string();
        let cutoff = now_ts - ttl_secs;
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM no_data_marks WHERE symbol = ? AND timeframe = ? AND last_checked >= ?",
                    params![symbol, timeframe, cutoff],
                    |_| Ok(()),
                )
                .is_ok();
            Ok(exists)
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::provider::{DataProvider, DataWriter, NegativeCache};
    use chrono::Utc;
    use rstest::rstest;

    fn make_bar(day: u32, open: f64, close: f64, volume: f64) -> Bar {
        // Fixed mid-week, mid-month base date (2026-08-05 is a Wednesday) so
        // weekly/monthly aggregation tests are deterministic. Utc::now()-relative
        // dates flaked whenever day+1/day+2 crossed an ISO week or month boundary
        // (e.g. CI failure in issue #75).
        Bar {
            time: chrono::NaiveDate::from_ymd_opt(2026, 8, 5)
                .expect("valid date")
                .and_hms_opt(0, 0, 0)
                .expect("valid datetime")
                .and_utc()
                + chrono::Duration::days(day as i64),
            open,
            high: open + 1.0,
            low: close - 1.0,
            close,
            volume,
        }
    }

    /// Verify that `save_bars` stores the correct symbol and timeframe,
    /// and that `fetch_bars` retrieves only those matching the same key.
    /// For "1w" / "1M", two consecutive daily bars aggregate to one bar.
    #[rstest]
    #[case("000001", "1d", 2)] // daily: 2 bars → 2 bars
    #[case("600519", "1w", 1)] // weekly: 2 daily bars same week → 1 bar
    #[case("AAPL", "1M", 1)] // monthly: 2 daily bars same month → 1 bar
    #[tokio::test]
    async fn save_and_fetch_preserves_symbol_and_timeframe(
        #[case] symbol: &str,
        #[case] timeframe: &str,
        #[case] expected_count: usize,
    ) {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let bars = vec![
            make_bar(1, 10.0, 10.5, 1000.0),
            make_bar(2, 10.5, 11.0, 2000.0),
        ];

        provider
            .save_bars(symbol, timeframe, &bars, true)
            .await
            .expect("save_bars failed");

        let fetched = provider
            .fetch_bars(symbol, timeframe, fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(
            fetched.len(),
            expected_count,
            "wrong count for {symbol}/{timeframe}"
        );
        if expected_count == 2 {
            assert_eq!(fetched[0].open, 10.0);
            assert_eq!(fetched[0].close, 10.5);
            assert_eq!(fetched[1].open, 10.5);
            assert_eq!(fetched[1].close, 11.0);
        } else {
            // Aggregated: 2 daily bars → 1 bar
            // open = first day's open (10.0), close = last day's close (11.0),
            // high = max(10+1, 10.5+1)=11.5, low = min(10.5-1, 11.0-1)=9.5
            assert_eq!(fetched[0].open, 10.0);
            assert_eq!(fetched[0].high, 11.5);
            assert_eq!(fetched[0].low, 9.5);
            assert_eq!(fetched[0].close, 11.0);
            assert_eq!(fetched[0].volume, 3000.0);
        }

        let other_sym = provider
            .fetch_bars(
                "NOT_EXIST",
                timeframe,
                fetch_all_start(),
                fetch_all_end(),
                "qfq",
            )
            .await
            .expect("fetch_bars for other symbol failed");
        assert!(
            other_sym.is_empty(),
            "should have no data for different symbol"
        );
    }

    fn fetch_all_start() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(0, 0).expect("valid epoch")
    }

    fn fetch_all_end() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end timestamp")
    }

    // -----------------------------------------------------------------------
    // NegativeCache tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn mark_no_data_then_is_no_data_returns_true() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let now = Utc::now().timestamp();
        let ttl = 7 * 24 * 3600;

        provider
            .mark_no_data("000003", "1d")
            .await
            .expect("mark_no_data failed");
        assert!(
            provider
                .is_no_data("000003", "1d", now, ttl)
                .await
                .expect("is_no_data failed")
        );
    }

    #[tokio::test]
    async fn is_no_data_returns_false_when_not_marked() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let now = Utc::now().timestamp();
        let ttl = 7 * 24 * 3600;

        assert!(
            !provider
                .is_no_data("000001", "1d", now, ttl)
                .await
                .expect("is_no_data failed")
        );
    }

    #[tokio::test]
    async fn is_no_data_returns_false_when_stale() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let stale_ts = Utc::now().timestamp() - 8 * 24 * 3600;
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute(
                "INSERT INTO no_data_marks (symbol, timeframe, last_checked) VALUES (?, ?, ?)",
                params!["000003", "1d", stale_ts],
            )
            .expect("insert stale mark");
        }

        let now = Utc::now().timestamp();
        let ttl = 7 * 24 * 3600;
        assert!(
            !provider
                .is_no_data("000003", "1d", now, ttl)
                .await
                .expect("is_no_data failed")
        );
    }

    // -----------------------------------------------------------------------
    // get_stored_range tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_stored_range_returns_none_when_empty() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let range = provider
            .get_stored_range("000001.SZ")
            .await
            .expect("get_stored_range failed");
        assert!(range.is_none());
    }

    #[tokio::test]
    async fn get_stored_range_returns_min_max_when_data_exists() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 1, 10).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 1, 12).expect("valid date");
        let d3 = NaiveDate::from_ymd_opt(2025, 1, 11).expect("valid date");

        // Insert out of order to verify MIN/MAX, not insertion order
        {
            let conn = provider.conn.lock().expect("mutex lock");
            let mut stmt = conn
                .prepare("INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, volume) VALUES (?, ?, 1, 2, 1, 2, 100)")
                .expect("prepare");
            stmt.execute(params!["000001", d2.format("%Y-%m-%d").to_string()])
                .expect("insert d2");
            stmt.execute(params!["000001", d1.format("%Y-%m-%d").to_string()])
                .expect("insert d1");
            stmt.execute(params!["000001", d3.format("%Y-%m-%d").to_string()])
                .expect("insert d3");
        }

        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range failed");
        assert_eq!(range, Some((d1, d2)));
    }

    // -----------------------------------------------------------------------
    // save_stock_daily tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_stock_daily_inserts_and_reads_back() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 3, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 3, 2).expect("valid date");

        let dr = |d: NaiveDate, o: f64, h: f64, l: f64, c: f64, v: f64, a: f64| DailyRecord {
            trade_date: d,
            open: o,
            high: h,
            low: l,
            close: c,
            adjclose: c,
            volume: v,
            amount: a,
        };

        let records = vec![
            dr(d1, 15.0, 16.0, 14.5, 15.5, 1000.0, 15000.0),
            dr(d2, 15.5, 17.0, 15.0, 16.5, 2000.0, 33000.0),
        ];

        provider
            .save_stock_daily("000001", &records, true)
            .await
            .expect("save_stock_daily failed");

        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range failed");
        assert_eq!(range, Some((d1, d2)));
    }

    #[tokio::test]
    async fn save_stock_daily_preserves_multiple_records() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 6, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 6, 2).expect("valid date");
        let d3 = NaiveDate::from_ymd_opt(2025, 6, 3).expect("valid date");

        let dr = |d: NaiveDate, o: f64, h: f64, l: f64, c: f64, v: f64, a: f64| DailyRecord {
            trade_date: d,
            open: o,
            high: h,
            low: l,
            close: c,
            adjclose: c,
            volume: v,
            amount: a,
        };

        let records = vec![
            dr(d2, 21.0, 22.0, 20.5, 21.5, 500.0, 10750.0),
            dr(d1, 20.0, 21.0, 19.5, 21.0, 300.0, 6300.0),
            dr(d3, 21.5, 23.0, 21.0, 22.0, 800.0, 17600.0),
        ];

        provider
            .save_stock_daily("000001", &records, true)
            .await
            .expect("save_stock_daily failed");

        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range failed");
        assert_eq!(range, Some((d1, d3)), "range should cover d1–d3");
    }

    #[tokio::test]
    async fn save_stock_daily_empty_records_does_nothing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .save_stock_daily("000001", &[], true)
            .await
            .expect("save_stock_daily with empty records failed");
        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range failed");
        assert!(range.is_none());
    }

    #[tokio::test]
    async fn save_stock_daily_skips_existing_when_overwrite_false() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 8, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 8, 2).expect("valid date");

        let dr = |d: NaiveDate, c: f64| DailyRecord {
            trade_date: d,
            open: c,
            high: c,
            low: c,
            close: c,
            adjclose: c,
            volume: 100.0,
            amount: 1000.0,
        };

        // First insert: write d1 with close=10.0
        provider
            .save_stock_daily("000001", &[dr(d1, 10.0)], true)
            .await
            .expect("first insert");

        // Second insert with overwrite=false: try to write d1 (close=99.0) and new d2
        provider
            .save_stock_daily("000001", &[dr(d1, 99.0), dr(d2, 20.0)], false)
            .await
            .expect("second insert with skip");

        // Verify: d1 should still have close=10.0 (skipped), d2 should have close=20.0
        let bars = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch");

        assert_eq!(bars.len(), 2, "expected 2 bars after skip-insert");
        assert!(
            (bars.iter().find(|b| b.close == 10.0).is_some()),
            "d1 should still have close=10.0 (skipped overwrite)"
        );
        assert!(
            (bars.iter().find(|b| b.close == 20.0).is_some()),
            "d2 should have close=20.0 (new record)"
        );
    }

    #[tokio::test]
    async fn save_stock_daily_overwrites_existing_when_overwrite_true() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 9, 1).expect("valid date");

        let dr = |c: f64| DailyRecord {
            trade_date: d1,
            open: c,
            high: c,
            low: c,
            close: c,
            adjclose: c,
            volume: 100.0,
            amount: 1000.0,
        };

        // First insert
        provider
            .save_stock_daily("000001", &[dr(10.0)], true)
            .await
            .expect("first insert");

        // Overwrite with overwrite=true
        provider
            .save_stock_daily("000001", &[dr(99.0)], true)
            .await
            .expect("overwrite insert");

        let bars = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch");

        assert_eq!(bars.len(), 1);
        assert!(
            (bars[0].close - 99.0).abs() < 0.01,
            "close should be 99.0 after overwrite"
        );
    }

    // -----------------------------------------------------------------------
    // stock_limit tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_limits_inserts_and_reads() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d = NaiveDate::from_ymd_opt(2025, 7, 1).expect("valid date");
        provider
            .save_limits(
                "000001",
                &[LimitRecord {
                    trade_date: d,
                    up_limit: 16.5,
                    down_limit: 13.5,
                }],
                true,
            )
            .await
            .expect("save_limits failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let (up, down): (f64, f64) = conn
            .query_row(
                "SELECT up_limit, down_limit FROM stock_limit WHERE symbol = '000001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        drop(conn);

        assert!((up - 16.5).abs() < 0.001);
        assert!((down - 13.5).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // adj_factor tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_adj_factors_inserts_and_reads_range() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2024, 6, 1).expect("valid date");

        provider
            .save_adj_factors(
                "000001",
                &[
                    AdjFactorRecord {
                        trade_date: d1,
                        adj_factor: 1.0,
                    },
                    AdjFactorRecord {
                        trade_date: d2,
                        adj_factor: 1.05,
                    },
                ],
                true,
            )
            .await
            .expect("save_adj_factors failed");

        let range = provider
            .get_adj_factor_range("000001")
            .await
            .expect("get_adj_factor_range failed");
        assert_eq!(range, Some((d1, d2)));
    }

    #[tokio::test]
    async fn get_adj_factor_range_returns_none_when_empty() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let range = provider
            .get_adj_factor_range("000001")
            .await
            .expect("get_adj_factor_range failed");
        assert!(range.is_none());
    }

    // -----------------------------------------------------------------------
    // Parquet-backed fetch_bars tests (ref #31)
    // -----------------------------------------------------------------------

    /// Create a `stock_daily.parquet` in a temp directory for testing.
    /// The parquet has columns: symbol, tradedate, open, high, low, close,
    /// adjclose, volume, amount. Returns the tempdir (must be kept alive)
    /// and the DuckDbProvider.
    type TestRow<'a> = (&'a str, f64, f64, f64, f64, f64, f64, f64);

    fn setup_parquet_provider(
        symbol: &str,
        rows: &[TestRow<'_>],
    ) -> (tempfile::TempDir, DuckDbProvider) {
        let tmp = tempfile::tempdir().expect("create temp dir");

        // Write test data to a single stock_daily.parquet using DuckDB
        let tmp_conn = duckdb::Connection::open_in_memory().expect("open temp conn");
        tmp_conn
            .execute_batch(
                "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
            )
            .expect("create temp table");
        let mut insert = tmp_conn
            .prepare("INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .expect("prepare insert");
        for (date_str, open, high, low, close, adjclose, volume, amount) in rows {
            insert
                .execute(params![
                    symbol, *date_str, *open, *high, *low, *close, *adjclose, *volume, *amount,
                ])
                .expect("insert row");
        }
        drop(insert);
        let parquet_path = tmp.path().join("stock_daily.parquet");
        let parquet_path_str = parquet_path.to_string_lossy();
        tmp_conn
            .execute_batch(&format!("COPY t TO '{parquet_path_str}' (FORMAT PARQUET)"))
            .expect("write parquet");
        drop(tmp_conn);

        let provider =
            DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("create provider");
        (tmp, provider)
    }

    /// Issue #31: DuckDbProvider with parquet_dir should read data from
    /// stock_daily.parquet on the first fetch (before any save_bars).
    #[tokio::test]
    async fn fetch_bars_reads_from_parquet_on_first_query() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[
                ("2020-01-02", 10.0, 11.0, 9.5, 10.5, 10.5, 1000.0, 10500.0),
                ("2020-01-03", 10.5, 12.0, 10.0, 11.5, 11.5, 2000.0, 23000.0),
                ("2020-01-06", 11.0, 11.8, 10.8, 11.2, 11.2, 1500.0, 16800.0),
            ],
        );

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");

        // Symbol is matched by column value in stock_daily.parquet
        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars should succeed");

        assert_eq!(bars.len(), 3, "should return 3 bars from parquet");
        assert!((bars[0].open - 10.0).abs() < 0.01);
        assert!((bars[2].close - 11.2).abs() < 0.01);
    }

    /// Verify that when parquet data exists, fetch_bars filters by date range.
    #[tokio::test]
    async fn fetch_bars_respects_date_range_from_parquet() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[
                ("2020-01-02", 10.0, 11.0, 9.5, 10.5, 10.5, 1000.0, 10500.0),
                ("2020-01-03", 10.5, 12.0, 10.0, 11.5, 11.5, 2000.0, 23000.0),
                ("2020-01-06", 11.0, 11.8, 10.8, 11.2, 11.2, 1500.0, 16800.0),
            ],
        );

        // Range that only covers the middle bar
        let start = chrono::DateTime::parse_from_rfc3339("2020-01-03T00:00:00Z")
            .expect("parse start")
            .with_timezone(&chrono::Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2020-01-03T23:59:59Z")
            .expect("parse end")
            .with_timezone(&chrono::Utc);

        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars should succeed");

        assert_eq!(bars.len(), 1, "should return only 1 bar in range");
        assert!((bars[0].open - 10.5).abs() < 0.01);
    }

    /// Verify that save_bars data takes priority over
    /// parquet data for the same dates.
    #[tokio::test]
    async fn save_bars_takes_priority_over_parquet() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[("2020-01-02", 10.0, 11.0, 9.5, 10.5, 10.5, 1000.0, 10500.0)],
        );

        let updated_bar = make_bar(2, 99.0, 100.0, 5000.0);
        provider
            .save_bars("000001", "1d", &[updated_bar], true)
            .await
            .expect("save_bars should succeed");

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");
        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars should succeed");

        // The saved bar (with 99.0 open) should be returned, not the parquet one (10.0)
        assert!(
            bars.iter().any(|b| (b.open - 99.0).abs() < 0.01),
            "saved bar with open=99.0 should be present"
        );
    }

    /// Verify that non-matching symbols in the parquet file return empty
    /// results without error (parameterized queries prevent SQL injection).
    #[tokio::test]
    async fn parquet_fallback_handles_non_matching_symbols() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[("2020-01-02", 10.0, 11.0, 9.5, 10.5, 10.5, 1000.0, 10500.0)],
        );

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");

        // Non-matching symbol should return empty, not error
        let result = provider
            .fetch_bars("'; DROP TABLE stock_daily; --", "1d", start, end, "qfq")
            .await;

        assert!(
            result.is_ok(),
            "parameterized query should handle any symbol safely"
        );
        assert!(
            result.unwrap().is_empty(),
            "non-matching symbol should return empty"
        );
    }

    /// After reading from parquet, data should be cached in-memory
    /// so subsequent queries hit DuckDB, not the filesystem.
    #[tokio::test]
    async fn fetch_bars_caches_parquet_data_in_memory() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[
                ("2020-01-02", 10.0, 11.0, 9.5, 10.5, 10.5, 1000.0, 10500.0),
                ("2020-01-06", 11.0, 11.8, 10.8, 11.2, 11.2, 1500.0, 16800.0),
            ],
        );

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");

        // First fetch — reads from parquet, should cache in-memory
        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("first fetch");
        assert_eq!(bars.len(), 2, "should read 2 bars from parquet");

        // After first fetch, stored range should reflect the cached data
        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range");
        assert!(
            range.is_some(),
            "data should be cached in-memory after parquet read"
        );

        let (min, max) = range.unwrap();
        assert_eq!(min, chrono::NaiveDate::from_ymd_opt(2020, 1, 2).unwrap());
        assert_eq!(max, chrono::NaiveDate::from_ymd_opt(2020, 1, 6).unwrap());
    }

    // -----------------------------------------------------------------------
    // Adjustment (复权) tests — fetch scales OHLC per adjust mode
    // (qfq/hfq/none, ref #345; qfq anchor = last valid adjclose/close ratio)
    // -----------------------------------------------------------------------

    /// Daily fetch from the in-memory table scales every OHLC price by the
    /// mode's factor (default qfq: ratio normalized by the last valid ratio).
    /// For this fixture (latest bar adjclose == close → ratio 1.0) the qfq
    /// factor equals the raw ratio, prices unchanged on the anchor bar.
    #[tokio::test]
    async fn fetch_bars_daily_scales_ohlc_by_adjclose_from_memory_table() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "INSERT INTO stock_daily
                     (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                 VALUES
                     ('000001', '2026-07-06', 9.0, 11.0, 8.0, 10.0, 8.0, 1000.0, 8000.0),
                     ('000001', '2026-07-07', 11.5, 13.0, 11.0, 12.0, 12.0, 2000.0, 24000.0)",
            )
            .expect("insert rows");
        }

        let bars = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(bars.len(), 2);
        // Older bar: factor = 8/10 = 0.8 → open 7.2, high 8.8, low 6.4, close 8.0.
        assert!((bars[0].open - 7.2).abs() < 1e-9, "scaled open");
        assert!((bars[0].high - 8.8).abs() < 1e-9, "scaled high");
        assert!((bars[0].low - 6.4).abs() < 1e-9, "scaled low");
        assert!(
            (bars[0].close - 8.0).abs() < 1e-9,
            "scaled close == adjclose"
        );
        assert_eq!(bars[0].volume, 1000.0, "volume passes through");
        // Latest bar: anchor factor 1.0 → unchanged.
        assert_eq!(bars[1].open, 11.5);
        assert_eq!(bars[1].close, 12.0);
    }

    /// Parquet fallback path (empty in-memory table → reads stock_daily.parquet)
    /// must also apply forward adjustment instead of dropping adjclose.
    #[tokio::test]
    async fn fetch_bars_scales_parquet_fallback_by_adjclose() {
        let (_tmp, provider) = setup_parquet_provider(
            "000001",
            &[
                ("2020-01-02", 10.0, 11.0, 9.5, 10.5, 8.4, 1000.0, 10500.0),
                ("2020-01-03", 10.5, 12.0, 10.0, 11.5, 11.5, 2000.0, 23000.0),
            ],
        );

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");

        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(bars.len(), 2);
        // Older bar: factor = 8.4/10.5 = 0.8 → open 8.0, high 8.8, low 7.6, close 8.4.
        assert!((bars[0].open - 8.0).abs() < 1e-9, "scaled open");
        assert!((bars[0].high - 8.8).abs() < 1e-9, "scaled high");
        assert!((bars[0].low - 7.6).abs() < 1e-9, "scaled low");
        assert!(
            (bars[0].close - 8.4).abs() < 1e-9,
            "scaled close == adjclose"
        );
        // Latest bar: anchor factor 1.0 → unchanged.
        assert!(
            (bars[1].close - 11.5).abs() < 1e-9,
            "anchor close unchanged"
        );
    }

    /// Weekly aggregation must scale daily bars FIRST, then aggregate — the
    /// week's MAX(high)/MIN(low) must be extremes of the *scaled* series,
    /// not of the raw series (ref #176).
    #[tokio::test]
    async fn fetch_bars_weekly_aggregates_scaled_daily_extremes() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "INSERT INTO stock_daily
                     (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                 VALUES
                     ('000001', '2026-07-06', 20.0, 25.0, 8.0, 10.0, 6.0, 300.0, 3000.0),
                     ('000001', '2026-07-07', 11.0, 13.0, 10.5, 12.0, 12.0, 500.0, 6000.0)",
            )
            .expect("insert rows");
        }

        let bars = provider
            .fetch_bars("000001", "1w", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(bars.len(), 1, "two bars in the same week → one weekly bar");
        let w = &bars[0];
        // Mon factor = 6/10 = 0.6 → scaled: open 12, high 15, low 4.8, close 6.
        // Weekly: open = FIRST(scaled open) = 12, high = MAX(15, 13) = 15,
        // low = MIN(4.8, 10.5) = 4.8, close = LAST(scaled close) = 12.
        // Unscaled extremes would be high 25 / low 8 — must NOT appear.
        assert!((w.open - 12.0).abs() < 1e-9, "open is first scaled open");
        assert!((w.high - 15.0).abs() < 1e-9, "high is max of scaled highs");
        assert!((w.low - 4.8).abs() < 1e-9, "low is min of scaled lows");
        assert!((w.close - 12.0).abs() < 1e-9, "close is last scaled close");
        assert_eq!(w.volume, 800.0, "volume sums unchanged");
    }

    // -----------------------------------------------------------------------
    // Timeframe aggregation tests — daily → weekly / monthly OHLCV resample
    // -----------------------------------------------------------------------

    /// Build a `Bar` with an explicit date string (UTC midnight).
    fn make_dated_bar(date: &str, open: f64, close: f64, volume: f64) -> Bar {
        let naive = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .expect("valid date")
            .and_hms_opt(0, 0, 0)
            .expect("valid time");
        Bar {
            time: naive.and_utc(),
            open,
            high: open + 1.0,
            low: close - 1.0,
            close,
            volume,
        }
    }

    /// Weekly aggregation: open=Mon open, high=week max, low=week min,
    /// close=Fri close, volume=week sum.
    #[rstest]
    #[case("1w", 2)] // 2 weeks → 2 weekly bars
    #[case("1M", 1)] // all in July → 1 monthly bar
    #[tokio::test]
    async fn fetch_bars_aggregates_daily_to_non_daily_timeframe(
        #[case] timeframe: &str,
        #[case] expected_count: usize,
    ) {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        // Week 1: 2026-07-06 (Mon) – 2026-07-10 (Fri)
        // Week 2: 2026-07-13 (Mon) – 2026-07-17 (Fri)
        let daily_bars = vec![
            make_dated_bar("2026-07-06", 10.0, 11.0, 100.0), // Mon
            make_dated_bar("2026-07-07", 11.0, 12.0, 200.0), // Tue
            make_dated_bar("2026-07-08", 12.0, 10.0, 300.0), // Wed
            make_dated_bar("2026-07-09", 10.0, 13.0, 400.0), // Thu
            make_dated_bar("2026-07-10", 13.0, 14.0, 500.0), // Fri
            make_dated_bar("2026-07-13", 15.0, 16.0, 100.0), // Mon
            make_dated_bar("2026-07-14", 16.0, 15.0, 200.0), // Tue
            make_dated_bar("2026-07-15", 15.0, 17.0, 300.0), // Wed
            make_dated_bar("2026-07-16", 17.0, 18.0, 400.0), // Thu
            make_dated_bar("2026-07-17", 18.0, 19.0, 500.0), // Fri
        ];

        provider
            .save_bars("000001", "1d", &daily_bars, true)
            .await
            .expect("save_bars failed");

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");
        let result = provider
            .fetch_bars("000001", timeframe, start, end, "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(
            result.len(),
            expected_count,
            "expected {expected_count} {timeframe} bar(s), got {}",
            result.len()
        );

        // Weekly checks
        if timeframe == "1w" {
            // Week 1: open=10.0, high=max(11,12,13,11,14)=14, low=min(10,11,9,12,13)=9,
            //          close=14.0, volume=1500
            let w1 = &result[0];
            assert_eq!(w1.open, 10.0, "week 1 open should be Mon's open");
            assert_eq!(w1.high, 14.0, "week 1 high should be max daily high");
            assert_eq!(w1.low, 9.0, "week 1 low should be min daily low");
            assert_eq!(w1.close, 14.0, "week 1 close should be Fri's close");
            assert_eq!(w1.volume, 1500.0, "week 1 volume should be sum");

            // Week 2: open=15.0, high=max(16,17,16,18,19)=19, low=min(15,14,16,17,18)=14,
            //          close=19.0, volume=1500
            let w2 = &result[1];
            assert_eq!(w2.open, 15.0, "week 2 open should be Mon's open");
            assert_eq!(w2.high, 19.0, "week 2 high should be max daily high");
            assert_eq!(w2.low, 14.0, "week 2 low should be min daily low");
            assert_eq!(w2.close, 19.0, "week 2 close should be Fri's close");
            assert_eq!(w2.volume, 1500.0, "week 2 volume should be sum");
        }

        // Monthly checks
        if timeframe == "1M" {
            let m1 = &result[0];
            // All 10 days in July 2026 → 1 monthly bar
            // open = first day's open (10.0)
            // high = max(all highs) = max(11,12,13,11,14,16,17,16,18,19) = 19.0
            // low = min(all lows) = min(10,11,9,12,13,15,14,16,17,18) = 9.0
            // close = last day's close (19.0)
            // volume = sum = 3000.0
            assert_eq!(m1.open, 10.0, "monthly open should be first day's open");
            assert_eq!(m1.high, 19.0, "monthly high should be max daily high");
            assert_eq!(m1.low, 9.0, "monthly low should be min daily low");
            assert_eq!(m1.close, 19.0, "monthly close should be last day's close");
            assert_eq!(m1.volume, 3000.0, "monthly volume should be sum");
        }
    }

    /// Daily timeframe returns raw bars unchanged.
    #[tokio::test]
    async fn fetch_bars_daily_returns_raw_daily_bars() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let daily_bars = vec![
            make_dated_bar("2026-07-06", 10.0, 11.0, 100.0),
            make_dated_bar("2026-07-07", 11.0, 12.0, 200.0),
        ];

        provider
            .save_bars("000001", "1d", &daily_bars, true)
            .await
            .expect("save_bars failed");

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");
        let result = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars failed");

        assert_eq!(result.len(), 2, "1d should return raw daily bars unchanged");
        assert_eq!(result[0].open, 10.0);
        assert_eq!(result[0].close, 11.0);
    }

    // -----------------------------------------------------------------------
    // new_file / execute_batch / table_has_rows / search_symbols tests
    // -----------------------------------------------------------------------

    #[test]
    fn new_file_creates_file_backed_duckdb_with_schema() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp.path().join("compass_test.db");
        let path_str = db_path.to_str().expect("valid UTF-8 path");
        let provider = DuckDbProvider::new_file(path_str).expect("new_file failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_daily", [], |row| row.get(0))
            .expect("query stock_daily");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn execute_batch_runs_multi_statement_sql() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .execute_batch(
                "CREATE TABLE batch_test (id INTEGER); \
                 INSERT INTO batch_test VALUES (1), (2), (3); \
                 CREATE TABLE batch_test_2 (name VARCHAR); \
                 INSERT INTO batch_test_2 VALUES ('hello');",
            )
            .await
            .expect("execute_batch failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM batch_test", [], |row| row.get(0))
            .expect("query batch_test");
        assert_eq!(count, 3);
        let name: String = conn
            .query_row("SELECT name FROM batch_test_2", [], |row| row.get(0))
            .expect("query batch_test_2");
        assert_eq!(name, "hello");
    }

    #[tokio::test]
    async fn table_has_rows_true_when_table_not_empty() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, volume) \
                 VALUES ('000001', '2025-01-01', 10, 11, 9, 10.5, 100)",
                [],
            )
            .expect("insert");
        }

        assert!(
            provider
                .table_has_rows("SELECT COUNT(*) FROM stock_daily")
                .await
                .expect("table_has_rows failed")
        );
    }

    #[tokio::test]
    async fn table_has_rows_false_when_table_empty() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        assert!(
            !provider
                .table_has_rows("SELECT COUNT(*) FROM stock_daily WHERE symbol = 'NONEXIST'")
                .await
                .expect("table_has_rows failed")
        );
    }

    #[tokio::test]
    async fn search_symbols_returns_empty_vec() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let result = provider
            .search_symbols("anything")
            .await
            .expect("search_symbols failed");
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // save_adj_factors: empty + overwrite=false
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_adj_factors_empty_records_does_nothing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .save_adj_factors("000001", &[], true)
            .await
            .expect("save_adj_factors with empty records should not error");

        let range = provider
            .get_adj_factor_range("000001")
            .await
            .expect("get_adj_factor_range failed");
        assert!(range.is_none());
    }

    #[tokio::test]
    async fn save_adj_factors_skips_existing_when_overwrite_false() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let d1 = NaiveDate::from_ymd_opt(2025, 1, 10).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 1, 11).expect("valid date");

        provider
            .save_adj_factors(
                "000001",
                &[AdjFactorRecord {
                    trade_date: d1,
                    adj_factor: 1.0,
                }],
                true,
            )
            .await
            .expect("first insert");

        provider
            .save_adj_factors(
                "000001",
                &[
                    AdjFactorRecord {
                        trade_date: d1,
                        adj_factor: 2.0,
                    },
                    AdjFactorRecord {
                        trade_date: d2,
                        adj_factor: 3.0,
                    },
                ],
                false,
            )
            .await
            .expect("second insert with skip");

        let conn = provider.conn.lock().expect("mutex lock");
        let (factor1,): (f64,) = conn
            .query_row(
                "SELECT adj_factor FROM stock_adj_factor WHERE symbol='000001' AND trade_date='2025-01-10'",
                [],
                |row| Ok((row.get(0)?,)),
            )
            .expect("query d1");
        let (factor2,): (f64,) = conn
            .query_row(
                "SELECT adj_factor FROM stock_adj_factor WHERE symbol='000001' AND trade_date='2025-01-11'",
                [],
                |row| Ok((row.get(0)?,)),
            )
            .expect("query d2");
        drop(conn);

        assert!(
            (factor1 - 1.0).abs() < 0.001,
            "d1 adj_factor should be 1.0 (skipped)"
        );
        assert!(
            (factor2 - 3.0).abs() < 0.001,
            "d2 adj_factor should be 3.0 (new)"
        );
    }

    // -----------------------------------------------------------------------
    // save_limits: empty + overwrite=false
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_limits_empty_records_does_nothing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .save_limits("000001", &[], true)
            .await
            .expect("save_limits with empty records should not error");

        let conn = provider.conn.lock().expect("mutex lock");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_limit WHERE symbol='000001'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        drop(conn);
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn save_limits_skips_existing_when_overwrite_false() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let d1 = NaiveDate::from_ymd_opt(2025, 3, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 3, 2).expect("valid date");

        provider
            .save_limits(
                "000001",
                &[LimitRecord {
                    trade_date: d1,
                    up_limit: 10.0,
                    down_limit: 9.0,
                }],
                true,
            )
            .await
            .expect("first insert");

        provider
            .save_limits(
                "000001",
                &[
                    LimitRecord {
                        trade_date: d1,
                        up_limit: 99.0,
                        down_limit: 88.0,
                    },
                    LimitRecord {
                        trade_date: d2,
                        up_limit: 20.0,
                        down_limit: 18.0,
                    },
                ],
                false,
            )
            .await
            .expect("second insert with skip");

        let conn = provider.conn.lock().expect("mutex lock");
        let (up1,): (f64,) = conn
            .query_row(
                "SELECT up_limit FROM stock_limit WHERE symbol='000001' AND trade_date='2025-03-01'",
                [],
                |row| Ok((row.get(0)?,)),
            )
            .expect("query d1");
        let (up2,): (f64,) = conn
            .query_row(
                "SELECT up_limit FROM stock_limit WHERE symbol='000001' AND trade_date='2025-03-02'",
                [],
                |row| Ok((row.get(0)?,)),
            )
            .expect("query d2");
        drop(conn);

        assert!(
            (up1 - 10.0).abs() < 0.001,
            "d1 up_limit should be 10.0 (skipped)"
        );
        assert!(
            (up2 - 20.0).abs() < 0.001,
            "d2 up_limit should be 20.0 (new)"
        );
    }

    // -----------------------------------------------------------------------
    // save_bars (DataWriter): empty + overwrite=false
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_bars_empty_does_nothing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .save_bars("000001", "1d", &[], true)
            .await
            .expect("save_bars with empty bars should not error");

        let range = provider
            .get_stored_range("000001")
            .await
            .expect("get_stored_range failed");
        assert!(range.is_none());
    }

    #[tokio::test]
    async fn save_bars_skips_existing_when_overwrite_false() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let bar1 = make_bar(1, 10.0, 10.5, 1000.0);
        let bar2 = make_bar(2, 20.0, 20.5, 2000.0);
        let bar1_updated = make_bar(1, 99.0, 99.5, 9999.0);

        provider
            .save_bars("000001", "1d", &[bar1], true)
            .await
            .expect("first save_bars");
        provider
            .save_bars("000001", "1d", &[bar1_updated, bar2], false)
            .await
            .expect("second save_bars with skip");

        let fetched = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch_bars");
        assert_eq!(fetched.len(), 2);
        assert!(
            fetched.iter().any(|b| (b.close - 10.5).abs() < 0.01),
            "bar1 close should still be 10.5 (skipped)"
        );
        assert!(
            fetched.iter().any(|b| (b.close - 20.5).abs() < 0.01),
            "bar2 close should be 20.5 (new)"
        );
    }

    // -----------------------------------------------------------------------
    // get_stored_range / get_adj_factor_range: date-parse error path
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_stored_range_parse_error_on_invalid_min_date() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "DROP TABLE stock_daily; \
                 CREATE TABLE stock_daily (\
                     symbol VARCHAR, trade_date VARCHAR,\
                     open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE,\
                     adjclose DOUBLE, volume DOUBLE, amount DOUBLE\
                 );",
            )
            .expect("recreate table");
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, adjclose, volume, amount) \
                 VALUES ('000001', 'not-a-date', 1, 2, 1, 2, 2, 100, 1000)",
                [],
            )
            .expect("insert bad date");
        }

        match provider.get_stored_range("000001").await {
            Err(DataError::Parse(msg)) => {
                assert!(msg.contains("invalid min date"), "unexpected error: {msg}");
            }
            other => panic!("expected DataError::Parse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_stored_range_parse_error_on_invalid_max_date() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "DROP TABLE stock_daily; \
                 CREATE TABLE stock_daily (\
                     symbol VARCHAR, trade_date VARCHAR,\
                     open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE,\
                     adjclose DOUBLE, volume DOUBLE, amount DOUBLE\
                 );",
            )
            .expect("recreate table");
            // "2025-01-01" < "z-invalid" lexicographically → MIN is valid, MAX is invalid
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, adjclose, volume, amount) \
                 VALUES ('000001', '2025-01-01', 1, 2, 1, 2, 2, 100, 1000)",
                [],
            )
            .expect("insert valid date");
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, adjclose, volume, amount) \
                 VALUES ('000001', 'z-invalid', 3, 4, 3, 4, 4, 200, 2000)",
                [],
            )
            .expect("insert invalid date");
        }

        match provider.get_stored_range("000001").await {
            Err(DataError::Parse(msg)) => {
                assert!(msg.contains("invalid max date"), "unexpected error: {msg}");
            }
            other => panic!("expected DataError::Parse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_adj_factor_range_parse_error_on_invalid_min_date() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "DROP TABLE stock_adj_factor; \
                 CREATE TABLE stock_adj_factor (\
                     symbol VARCHAR, trade_date VARCHAR,\
                     adj_factor DOUBLE\
                 );",
            )
            .expect("recreate table");
            conn.execute(
                "INSERT INTO stock_adj_factor (symbol, trade_date, adj_factor) \
                 VALUES ('000001', 'bad-date-format', 1.0)",
                [],
            )
            .expect("insert bad date");
        }

        match provider.get_adj_factor_range("000001").await {
            Err(DataError::Parse(msg)) => {
                assert!(msg.contains("invalid min date"), "unexpected error: {msg}");
            }
            other => panic!("expected DataError::Parse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_adj_factor_range_parse_error_on_invalid_max_date() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "DROP TABLE stock_adj_factor; \
                 CREATE TABLE stock_adj_factor (\
                     symbol VARCHAR, trade_date VARCHAR,\
                     adj_factor DOUBLE\
                 );",
            )
            .expect("recreate table");
            conn.execute(
                "INSERT INTO stock_adj_factor (symbol, trade_date, adj_factor) \
                 VALUES ('000001', '2025-01-01', 1.0)",
                [],
            )
            .expect("insert valid date");
            conn.execute(
                "INSERT INTO stock_adj_factor (symbol, trade_date, adj_factor) \
                 VALUES ('000001', 'z-invalid', 2.0)",
                [],
            )
            .expect("insert invalid date");
        }

        match provider.get_adj_factor_range("000001").await {
            Err(DataError::Parse(msg)) => {
                assert!(msg.contains("invalid max date"), "unexpected error: {msg}");
            }
            other => panic!("expected DataError::Parse, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // fetch_bars: TIMESTAMP trade_date exercises date_str_to_utc format parsers
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_bars_parses_timestamp_dates() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch(
                "DROP TABLE stock_daily; \
                 CREATE TABLE stock_daily (\
                     symbol VARCHAR, trade_date TIMESTAMP, \
                     open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE,\
                     adjclose DOUBLE, volume DOUBLE, amount DOUBLE\
                 );",
            )
            .expect("recreate table");
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, adjclose, volume, amount) \
                 VALUES ('000001', TIMESTAMP '2026-03-15 10:30:00', 10, 11, 9, 10.5, 10.5, 1000, 10500)",
                [],
            )
            .expect("insert timestamp");
            conn.execute(
                "INSERT INTO stock_daily (symbol, trade_date, open, high, low, close, adjclose, volume, amount) \
                 VALUES ('000001', TIMESTAMP '2026-03-16 14:45:30.500', 11, 12, 10, 11.5, 11.5, 2000, 23000)",
                [],
            )
            .expect("insert timestamp with fractional seconds");
        }

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");
        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars from timestamp table");
        assert_eq!(bars.len(), 2);
    }

    // -----------------------------------------------------------------------
    // fetch_bars: unknown timeframe + parquet_dir set but file missing
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_bars_unknown_timeframe_falls_back_to_day() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let bars = vec![
            make_dated_bar("2026-07-06", 10.0, 11.0, 100.0),
            make_dated_bar("2026-07-07", 11.0, 12.0, 200.0),
        ];
        provider
            .save_bars("000001", "1d", &bars, true)
            .await
            .expect("save_bars failed");

        let result = provider
            .fetch_bars("000001", "4h", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("fetch_bars with unknown timeframe should not error");
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn fetch_bars_returns_empty_when_parquet_dir_set_but_file_missing() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let provider =
            DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("create provider");

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end");
        let bars = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("fetch_bars should not error");
        assert!(bars.is_empty());
    }

    // -----------------------------------------------------------------------
    // Issue #345 — adjust mode (qfq/hfq/none) adversarial tests
    //
    // Contract under attack (plan §1.5):
    //   fetch_bars(symbol, timeframe, start, end, adjust: &str)
    //   adjust ∈ {"qfq","hfq","none"}; unknown values fall back to "qfq".
    //   none → factor 1.0; hfq → factor = ratio (ratio = adjclose/close,
    //   invalid rows → 1.0); qfq → factor = ratio / r_anchor where r_anchor is
    //   the ratio of the LAST valid ratio row (no valid row → all 1.0).
    //   Aggregation (1w/1M): scale first, then aggregate; qfq r_anchor is
    //   computed at the daily layer.
    // -----------------------------------------------------------------------

    /// Field-wise comparison (Bar does not necessarily implement PartialEq).
    /// Macro accepts an optional context message for diagnostics.
    macro_rules! assert_bars_eq {
        ($a:expr, $b:expr) => {
            assert_bars_eq_impl($a, $b, "")
        };
        ($a:expr, $b:expr, $msg:expr) => {
            assert_bars_eq_impl($a, $b, $msg)
        };
    }

    fn assert_bars_eq_impl(a: &Bar, b: &Bar, msg: &str) {
        assert!(
            (a.time - b.time).num_milliseconds().abs() < 1,
            "{msg}: time differs: {} vs {}",
            a.time,
            b.time
        );
        assert!(
            (a.open - b.open).abs() < 1e-9,
            "{msg}: open differs: {} vs {}",
            a.open,
            b.open
        );
        assert!(
            (a.high - b.high).abs() < 1e-9,
            "{msg}: high differs: {} vs {}",
            a.high,
            b.high
        );
        assert!(
            (a.low - b.low).abs() < 1e-9,
            "{msg}: low differs: {} vs {}",
            a.low,
            b.low
        );
        assert!(
            (a.close - b.close).abs() < 1e-9,
            "{msg}: close differs: {} vs {}",
            a.close,
            b.close
        );
        assert_eq!(
            a.volume, b.volume,
            "{msg}: volume differs: {} vs {}",
            a.volume, b.volume
        );
    }

    async fn fetch_adjust(provider: &DuckDbProvider, adjust: &str) -> Result<Vec<Bar>, DataError> {
        provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), adjust)
            .await
    }

    /// One explicit daily row: (date, open, high, low, close, adjclose, volume).
    type DailyRowFixture = (&'static str, f64, f64, f64, f64, Option<f64>, f64);

    /// Insert explicit daily rows. `adjclose: None` writes a SQL NULL, which
    /// is the SZ300683-style hole the adjust logic must survive.
    fn insert_daily_rows(conn: &duckdb::Connection, symbol: &str, rows: &[DailyRowFixture]) {
        let mut stmt = conn
            .prepare(
                "INSERT INTO stock_daily
                 (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .expect("prepare insert");
        for (date, open, high, low, close, adjclose, volume) in rows {
            stmt.execute(duckdb::params![
                symbol, date, open, high, low, close, adjclose, volume, 0.0
            ])
            .expect("insert row");
        }
    }

    /// Empty sequence — no rows in the range must yield an empty Vec for all
    /// three modes, never a panic (nor a fabricated single bar).
    #[tokio::test]
    async fn fetch_bars_no_rows_returns_empty_for_all_adjust_modes() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        for adjust in ["qfq", "hfq", "none"] {
            let bars = provider
                .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), adjust)
                .await
                .expect("fetch_bars should not error on an empty result");
            assert!(
                bars.is_empty(),
                "adjust={adjust}: empty range must return empty bars, got {}",
                bars.len()
            );
        }
    }

    /// Range that does not cover the stored rows — same empty-seq contract.
    #[tokio::test]
    async fn fetch_bars_range_without_rows_returns_empty_for_all_adjust_modes() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[("2026-07-06", 9.0, 12.0, 8.0, 10.0, Some(8.0), 100.0)],
            );
        }

        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(1_000_000_000, 0).expect("valid end");
        for adjust in ["qfq", "hfq", "none"] {
            let bars = provider
                .fetch_bars("000001", "1d", start, end, adjust)
                .await
                .expect("fetch_bars should not error");
            assert!(
                bars.is_empty(),
                "adjust={adjust}: out-of-range must be empty"
            );
        }
    }

    /// Single-bar sequence: no notion of "latest" — qfq must normalize to
    /// factor 1.0 (r_anchor == the bar itself), while hfq scales by ratio.
    #[tokio::test]
    async fn fetch_bars_single_bar_qfq_equals_none_but_hfq_scales() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                // (date, open, high, low, close, adjclose=15 → ratio 1.5, volume)
                &[("2026-07-06", 9.0, 12.0, 8.0, 10.0, Some(15.0), 100.0)],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 1);
        // qfq: r_anchor = 1.5 (only valid ratio) → factor = 1.5/1.5 = 1.0.
        assert_eq!(qfq[0].close, 10.0, "single-bar qfq must be factor 1.0");
        assert_eq!(qfq[0].open, 9.0);
        // none: factor 1.0 everywhere.
        assert_bars_eq!(&qfq[0], &none[0]);
        // hfq: factor = 1.5 → every price scaled.
        assert!((hfq[0].close - 15.0).abs() < 1e-9, "hfq single bar close");
        assert!((hfq[0].open - 13.5).abs() < 1e-9, "hfq single bar open");
        assert!((hfq[0].high - 18.0).abs() < 1e-9, "hfq single bar high");
        assert!((hfq[0].low - 12.0).abs() < 1e-9, "hfq single bar low");
        assert_eq!(hfq[0].volume, 100.0, "volume never scaled");
    }

    /// All rows close <= 0: every ratio is invalid, so all three modes must
    /// degenerate to factor 1.0 with finite prices — never NaN/Inf.
    #[tokio::test]
    async fn fetch_bars_non_positive_close_three_modes_identical_and_finite() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    ("2026-07-06", 1.0, 2.0, -1.0, 0.0, Some(5.0), 100.0),
                    ("2026-07-07", -4.0, -3.0, -6.0, -5.0, None, 200.0),
                ],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 2);
        for bars in [&qfq, &hfq, &none] {
            for b in bars {
                assert!(
                    b.open.is_finite()
                        && b.high.is_finite()
                        && b.low.is_finite()
                        && b.close.is_finite(),
                    "non-finite price leaked: {b:?}"
                );
            }
        }
        assert_bars_eq!(&qfq[0], &none[0]);
        assert_bars_eq!(&qfq[1], &none[1]);
        assert_bars_eq!(&hfq[0], &none[0]);
        assert_bars_eq!(&hfq[1], &none[1]);
        // Unchanged (factor 1.0), close==0/‑5 preserved exactly.
        assert_eq!(qfq[0].close, 0.0);
        assert_eq!(qfq[1].close, -5.0);
    }

    /// All adjclose NULL (SZ300683-style): no valid ratio exists → qfq has no
    /// anchor and must fall back to factor 1.0 for every row; the three modes
    /// are then output-identical to the raw series.
    #[tokio::test]
    async fn fetch_bars_all_null_adjclose_three_modes_identical() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    ("2026-07-06", 9.0, 12.0, 8.0, 10.0, None, 100.0),
                    ("2026-07-07", 19.0, 22.0, 18.0, 20.0, None, 200.0),
                    ("2026-07-08", 29.0, 32.0, 28.0, 30.0, None, 300.0),
                ],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 3);
        for i in 0..3 {
            assert_bars_eq!(&qfq[i], &none[i], &format!("row {i} qfq vs none"));
            assert_bars_eq!(&hfq[i], &none[i], &format!("row {i} hfq vs none"));
            assert!((none[i].close - [10.0, 20.0, 30.0][i]).abs() < 1e-9);
        }
    }

    /// Trailing adjclose NULL: the qfq anchor must move forward to the last
    /// VALID ratio row (pre-anchor rows scale, the anchor row and the NULL
    /// tail both get factor 1.0). Anchoring on the raw last row instead would
    /// scale row 2 by 0.9/1.0 = 0.9 → close 10.8 ≠ 12.0.
    #[tokio::test]
    async fn fetch_bars_tail_null_adjclose_qfq_anchor_moves_to_last_valid() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    // ratio 0.8
                    ("2026-07-06", 9.0, 12.0, 8.0, 10.0, Some(8.0), 100.0),
                    // ratio 0.9 — the LAST valid ratio → qfq anchor
                    ("2026-07-07", 11.0, 13.0, 10.5, 12.0, Some(10.8), 200.0),
                    // NULL tail — invalid ratio
                    ("2026-07-08", 13.0, 14.0, 12.0, 14.0, None, 300.0),
                ],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");

        assert_eq!(qfq.len(), 3);
        // qfq factors: [0.8/0.9, 1.0, 1.0]
        assert!(
            (qfq[0].close - 10.0 * 0.8 / 0.9).abs() < 1e-9,
            "pre-anchor close scaled"
        );
        assert_eq!(qfq[1].close, 12.0, "anchor row must be factor 1.0");
        assert_eq!(
            qfq[2].close, 14.0,
            "NULL tail row must be factor 1.0, not scaled by 1.0/0.9"
        );
        assert_eq!(qfq[2].open, 13.0, "NULL tail open unchanged");
        // hfq factors: [0.8, 0.9, 1.0] — independent of any anchor.
        assert!((hfq[0].close - 8.0).abs() < 1e-9);
        assert!((hfq[1].close - 10.8).abs() < 1e-9);
        assert_eq!(hfq[2].close, 14.0);
    }

    /// Head-only adjclose NULL: the head invalid row stays factor 1.0 while
    /// later rows normalize; with r_anchor == 1.0 the qfq and hfq outputs
    /// coincide (both ≠ none).
    #[tokio::test]
    async fn fetch_bars_head_null_adjclose_qfq_normalizes_rest() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    ("2026-07-06", 9.0, 12.0, 8.0, 10.0, None, 100.0), // ratio invalid
                    ("2026-07-07", 19.0, 22.0, 18.0, 20.0, Some(15.0), 200.0), // 0.75
                    ("2026-07-08", 29.0, 32.0, 28.0, 30.0, Some(30.0), 300.0), // 1.0 → anchor
                ],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 3);
        // qfq factors: [1.0, 0.75, 1.0].
        assert_eq!(qfq[0].close, 10.0, "head NULL row stays unscaled");
        assert!(
            (qfq[1].close - 15.0).abs() < 1e-9,
            "middle row scaled by 0.75"
        );
        assert_eq!(qfq[2].close, 30.0, "anchor row is scaled to 1.0");
        // hfq: factors [1.0, 0.75, 1.0] — identical to qfq because anchor == 1.0.
        assert_bars_eq!(&qfq[0], &hfq[0]);
        assert_bars_eq!(&qfq[1], &hfq[1]);
        assert_bars_eq!(&qfq[2], &hfq[2]);
        // ... but both differ from none on row 1.
        assert!((none[1].close - 20.0).abs() < 1e-9);
        assert!(
            (qfq[1].close - none[1].close).abs() > 1e-3,
            "qfq must differ from none"
        );
    }

    /// Index-shape series (SH000001: adjclose == close, ratio == 1.0): all
    /// three modes must be output-identical, and identical to the raw series.
    #[tokio::test]
    async fn fetch_bars_ratio_one_index_shape_three_modes_identical() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    (
                        "2026-07-06",
                        2990.0,
                        3010.0,
                        2980.0,
                        3000.0,
                        Some(3000.0),
                        100.0,
                    ),
                    (
                        "2026-07-07",
                        3005.0,
                        3020.0,
                        2990.0,
                        3010.0,
                        Some(3010.0),
                        200.0,
                    ),
                    (
                        "2026-07-08",
                        3015.0,
                        3030.0,
                        3002.0,
                        3020.0,
                        Some(3020.0),
                        300.0,
                    ),
                ],
            );
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 3);
        for i in 0..3 {
            assert_bars_eq!(&qfq[i], &none[i], &format!("row {i} qfq vs none"));
            assert_bars_eq!(&hfq[i], &none[i], &format!("row {i} hfq vs none"));
            assert!(
                (none[i].close - [3000.0, 3010.0, 3020.0][i]).abs() < 1e-9,
                "raw close must pass through untouched"
            );
        }
    }

    /// Non-finite adjclose (NaN / +Inf) must invalidate the ratio, collapse to
    /// factor 1.0, and never panic or leak non-finite prices.
    #[tokio::test]
    async fn fetch_bars_non_finite_adjclose_never_panics() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            let mut stmt = conn
                .prepare(
                    "INSERT INTO stock_daily
                     (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .expect("prepare insert");
            stmt.execute(duckdb::params![
                "000001",
                "2026-07-06",
                9.0,
                12.0,
                8.0,
                10.0,
                f64::NAN,
                100.0,
                0.0
            ])
            .expect("insert NaN adjclose");
            stmt.execute(duckdb::params![
                "000001",
                "2026-07-07",
                19.0,
                22.0,
                18.0,
                20.0,
                f64::INFINITY,
                200.0,
                0.0
            ])
            .expect("insert Inf adjclose");
        }

        let qfq = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let hfq = fetch_adjust(&provider, "hfq").await.expect("hfq fetch");
        let none = fetch_adjust(&provider, "none").await.expect("none fetch");

        assert_eq!(qfq.len(), 2);
        for bars in [&qfq, &hfq, &none] {
            for b in bars {
                assert!(
                    b.open.is_finite()
                        && b.high.is_finite()
                        && b.low.is_finite()
                        && b.close.is_finite(),
                    "non-finite price leaked from non-finite adjclose: {b:?}"
                );
            }
        }
        assert_bars_eq!(&qfq[0], &none[0]);
        assert_bars_eq!(&hfq[1], &none[1]);
        assert_eq!(qfq[0].close, 10.0);
        assert_eq!(qfq[1].close, 20.0);
    }

    /// Unknown adjust values (including case variants, whitespace and
    /// Unicode) must fall back to "qfq" — output identical to the explicit
    /// "qfq" fetch. The fixture has ratio != 1.0 so a wrong fallback to
    /// "none" (or a blanket factor 1.0) is caught.
    #[rstest]
    #[case("invalid")]
    #[case("")]
    #[case("QFQ")]
    #[case("None")]
    #[case("qFq")]
    #[case(" none")]
    #[case("hfq ")]
    #[case("前复权")]
    #[case("adjust")]
    #[case("⚠️")]
    #[tokio::test]
    async fn fetch_bars_unknown_adjust_falls_back_to_qfq(#[case] adjust: &str) {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    ("2026-07-06", 9.0, 12.0, 8.0, 10.0, Some(8.0), 100.0), // ratio 0.8
                    ("2026-07-07", 11.0, 13.0, 10.5, 12.0, Some(10.8), 200.0), // ratio 0.9
                    ("2026-07-08", 13.0, 14.0, 12.0, 14.0, None, 300.0),
                ],
            );
        }

        let expected = fetch_adjust(&provider, "qfq").await.expect("qfq fetch");
        let actual = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), adjust)
            .await
            .unwrap_or_else(|e| panic!("unknown adjust {adjust:?} must not error: {e}"));

        assert_eq!(expected.len(), actual.len(), "adjust={adjust:?}");
        for (e, a) in expected.iter().zip(actual.iter()) {
            assert_bars_eq!(e, a, &format!("adjust={adjust:?} must behave like qfq"));
        }
    }

    /// Weekly aggregation qfq: scale first, then aggregate; the last weekly
    /// close must equal the daily qfq close of the last valid row (anchor
    /// invariance) while hfq/none differ. Hand-computed values from fixture
    /// ratios [0.4, 0.5, 0.8, 2.0] (r_anchor = 2.0).
    #[tokio::test]
    async fn fetch_bars_weekly_three_modes_scale_before_aggregate() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    // week 1 (2026-07-06 Mon .. 07-09 Thu)
                    ("2026-07-06", 20.0, 25.0, 8.0, 10.0, Some(4.0), 300.0), // 0.4
                    ("2026-07-08", 11.0, 13.0, 10.5, 12.0, Some(6.0), 400.0), // 0.5
                    ("2026-07-09", 13.0, 14.0, 12.0, 13.0, Some(10.4), 500.0), // 0.8
                    // week 2 (2026-07-13 Mon) — last valid ratio → r_anchor
                    ("2026-07-13", 14.0, 16.0, 13.0, 15.0, Some(30.0), 600.0), // 2.0
                ],
            );
        }

        let qfq = provider
            .fetch_bars("000001", "1w", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("qfq weekly fetch");
        let hfq = provider
            .fetch_bars("000001", "1w", fetch_all_start(), fetch_all_end(), "hfq")
            .await
            .expect("hfq weekly fetch");
        let none = provider
            .fetch_bars("000001", "1w", fetch_all_start(), fetch_all_end(), "none")
            .await
            .expect("none weekly fetch");

        assert_eq!(qfq.len(), 2, "two weeks → two weekly bars");
        assert_eq!(hfq.len(), 2);
        assert_eq!(none.len(), 2);

        // qfq factors [0.2, 0.25, 0.4, 1.0]:
        // W1 scaled extremes: high = MAX(5.0, 3.25, 5.6) = 5.6,
        // low = MIN(1.6, 2.625, 4.8) = 1.6, open = FIRST = 4.0,
        // close = LAST = 5.2. A "aggregate-then-scale" implementation would
        // yield high 5.0 (= 25 * 0.2) instead — caught here.
        assert!((qfq[0].open - 4.0).abs() < 1e-9, "qfq W1 open");
        assert!(
            (qfq[0].high - 5.6).abs() < 1e-9,
            "qfq W1 high (scale-then-aggregate)"
        );
        assert!((qfq[0].low - 1.6).abs() < 1e-9, "qfq W1 low");
        assert!((qfq[0].close - 5.2).abs() < 1e-9, "qfq W1 close");
        assert_eq!(qfq[0].volume, 1200.0);
        // W2 anchor: factor 1.0 → close 15 (latest = current price).
        assert!(
            (qfq[1].close - 15.0).abs() < 1e-9,
            "qfq last weekly close == raw latest"
        );
        assert_eq!(qfq[1].volume, 600.0);

        // hfq factors [0.4, 0.5, 0.8, 2.0]: W1 close = 10.4, W2 close = 30.
        assert!((hfq[0].close - 10.4).abs() < 1e-9, "hfq W1 close");
        assert!(
            (hfq[1].close - 30.0).abs() < 1e-9,
            "hfq W2 close differs from qfq"
        );

        // none: raw extremes; W1 close = 13.
        assert!((none[0].close - 13.0).abs() < 1e-9, "none W1 close");
        assert!((none[1].close - 15.0).abs() < 1e-9, "none W2 close");

        // The three modes must be mutually distinguishable in the aggregate.
        assert!(
            (qfq[0].close - none[0].close).abs() > 1e-3,
            "qfq ≠ none weekly"
        );
        assert!(
            (hfq[0].close - none[0].close).abs() > 1e-3,
            "hfq ≠ none weekly"
        );
    }

    /// Contract-ambiguity probe (reported, not assumed): with a trailing NULL
    /// adjclose row, plan §1.5 says invalid rows get factor 1.0 (daily) while
    /// the aggregate SQL says scale ÷ r_anchor for every row (weekly). A
    /// faithful double-formula implementation would scale the weekly last
    /// close by 1.0/0.5 = 2.0 → 24, contradicting the "last valid bar close
    /// unchanged" equivalence. This test pins the coherent reading: invalid
    /// rows stay factor 1.0 on the aggregate path too.
    #[tokio::test]
    async fn fetch_bars_weekly_tail_null_adjclose_equals_daily_qfq() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    ("2026-07-06", 9.0, 12.0, 8.0, 10.0, Some(5.0), 100.0), // ratio 0.5 → anchor
                    ("2026-07-07", 11.0, 13.0, 10.0, 12.0, None, 200.0),    // NULL tail
                ],
            );
        }

        let daily = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("daily qfq fetch");
        let weekly = provider
            .fetch_bars("000001", "1w", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("weekly qfq fetch");

        assert_eq!(daily.len(), 2);
        assert_eq!(weekly.len(), 1);
        assert_eq!(daily[1].close, 12.0, "daily NULL tail stays factor 1.0");
        assert!(
            (weekly[0].close - daily[1].close).abs() < 1e-9,
            "weekly last close must equal daily last close (got {} vs {}); \
             the aggregate SQL must NOT divide the invalid-row fallback by r_anchor \
             (plan §1.5: invalid rows factor 1.0, equivalence of last valid bar)",
            weekly[0].close,
            daily[1].close
        );
    }

    /// Adversarial boundary: dropping the table must surface a DataError, not
    /// a panic, for each adjust mode (error-path propagation).
    #[tokio::test]
    async fn fetch_bars_dropped_table_returns_error_not_panic() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch("DROP TABLE stock_daily")
                .expect("drop table");
        }

        for adjust in ["qfq", "hfq", "none"] {
            let result = provider
                .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end(), adjust)
                .await;
            assert!(
                result.is_err(),
                "adjust={adjust}: dropped table must surface Err, got Ok"
            );
        }
    }

    /// Resource-exhaustion + linearity probe: 50k rows through qfq/hfq/none
    /// must complete without OOM or panic, and qfq must finish in bounded
    /// time (an O(n²) anchor search over 50k rows is ~2.5e9 steps and would
    /// blow the 30s guard; the linear path takes well under 1s).
    #[tokio::test]
    async fn fetch_bars_50k_rows_three_modes_bounded_and_linear() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        const N: u32 = 50_000;

        {
            let conn = provider.conn.lock().expect("mutex lock");
            conn.execute_batch("BEGIN TRANSACTION").expect("begin");
            let mut stmt = conn
                .prepare(
                    "INSERT INTO stock_daily
                     (symbol, trade_date, open, high, low, close, adjclose, volume, amount)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .expect("prepare bulk insert");
            let base = chrono::NaiveDate::from_ymd_opt(2000, 1, 3).expect("valid date");
            for i in 0..N {
                let close = 10.0 + (i % 997) as f64;
                let date = base + chrono::Duration::days(i as i64);
                let adjclose: Option<f64> = if i % 3 == 0 { None } else { Some(close * 1.5) };
                stmt.execute(duckdb::params![
                    "000001",
                    date.format("%Y-%m-%d").to_string(),
                    close - 1.0,
                    close + 1.0,
                    close - 2.0,
                    close,
                    adjclose,
                    100.0,
                    0.0
                ])
                .expect("bulk insert row");
            }
            conn.execute_batch("COMMIT").expect("commit");
        }

        let last_raw_close = 10.0 + ((N - 1) % 997) as f64; // row N-1 has adjclose (N-1)%3 != 0

        // 50k daily rows span ~137 years from 2000-01-03 — beyond the 4e9
        // second (≈2096) fetch_all_end. Use a wider end so the range covers
        // every inserted row.
        let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
        let end = chrono::DateTime::from_timestamp(6_000_000_000, 0).expect("valid end");

        let t0 = std::time::Instant::now();
        let qfq = provider
            .fetch_bars("000001", "1d", start, end, "qfq")
            .await
            .expect("qfq 50k fetch");
        let elapsed = t0.elapsed();

        assert_eq!(qfq.len(), N as usize, "all rows returned");
        assert!(
            elapsed.as_secs() < 30,
            "qfq over 50k rows took {elapsed:?} — an O(n²) anchor scan is suspected"
        );
        let last = qfq.last().expect("non-empty");
        assert!(
            (last.close - last_raw_close).abs() < 1e-9,
            "last row (valid ratio) must be factor 1.0, got {} vs {}",
            last.close,
            last_raw_close
        );
        for b in qfq.iter() {
            assert!(b.close.is_finite(), "non-finite close in 50k qfq output");
        }

        // Resource check: hfq and none over the same 50k rows also complete.
        let hfq = provider
            .fetch_bars("000001", "1d", start, end, "hfq")
            .await
            .expect("hfq 50k fetch");
        let none = provider
            .fetch_bars("000001", "1d", start, end, "none")
            .await
            .expect("none 50k fetch");
        assert_eq!(hfq.len(), N as usize);
        assert_eq!(none.len(), N as usize);
        assert_eq!(
            none.last().expect("last").close,
            last_raw_close,
            "none passes raw through"
        );
        assert!(
            (hfq.last().expect("last").close - last_raw_close * 1.5).abs() < 1e-9,
            "hfq last row scales by its own ratio"
        );
    }

    // -----------------------------------------------------------------------
    // Issue #345 — requirement-acceptance tests (happy path, plan §4/§1.5).
    // Adversarial coverage (edge/NULL/non-finite/50k/aggregate-edge) lives
    // in the tests above; these verify the THREE-MODE NUMERIC CONTRACT on a
    // clean, fully-valid series and the aggregation happy path.
    //
    // SZ002832 shape (first row ratio = 1.0, last row ratio ≈ 6.0123 with
    // close 25.11 / adjclose 150.97): real-file verification is NOT a
    // committed test (CI must not depend on /data/compass-data/parquet_data
    // — ref #236); the same shape is reproduced with in-memory DuckDB
    // fixture rows and hand-computed expectations.
    // -----------------------------------------------------------------------

    /// Given a fully-valid post-adjusted series (first day ratio 1.0, last
    /// day ratio ≈ 6.0123 — SZ002832 shape), all three modes must yield the
    /// plan §1.5 numeric contract:
    /// - qfq: latest bar close == raw close (current price); earlier rows
    ///   scale by ratio / r_anchor (r_anchor = 150.97 / 25.11);
    /// - hfq: close == raw close × ratio (i.e. the adjclose value);
    ///   the first day (ratio 1.0) stays untouched;
    /// - none: raw OHLC passes through untouched.
    /// Every mode scales open/high/low by the same factor as close; volume
    /// is NEVER scaled.
    #[tokio::test]
    async fn fetch_bars_three_adjust_modes_happy_path_numeric_values() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "SZ002832",
                &[
                    // first day — ratio 1.0
                    ("2026-07-06", 24.0, 25.5, 23.5, 25.11, Some(25.11), 1000.0),
                    // ratio 2.0
                    ("2026-07-07", 26.0, 28.0, 25.5, 27.0, Some(54.0), 2000.0),
                    // last valid row — ratio = 150.97 / 25.11 ≈ 6.0123 → qfq anchor
                    ("2026-07-08", 25.0, 26.0, 24.8, 25.11, Some(150.97), 3000.0),
                ],
            );
        }

        let r_anchor = 150.97 / 25.11; // ≈ 6.0123... (hand-computed per contract)
        let start = fetch_all_start();
        let end = fetch_all_end();

        let qfq = provider
            .fetch_bars("SZ002832", "1d", start, end, "qfq")
            .await
            .expect("qfq fetch");
        let hfq = provider
            .fetch_bars("SZ002832", "1d", start, end, "hfq")
            .await
            .expect("hfq fetch");
        let none = provider
            .fetch_bars("SZ002832", "1d", start, end, "none")
            .await
            .expect("none fetch");

        assert_eq!(qfq.len(), 3, "qfq must return all three rows");
        assert_eq!(hfq.len(), 3, "hfq must return all three rows");
        assert_eq!(none.len(), 3, "none must return all three rows");

        // --- qfq: latest bar close == raw close (current price) ---
        assert!(
            (qfq[2].close - 25.11).abs() < 1e-9,
            "qfq latest bar must equal the raw close (current price), got {}",
            qfq[2].close
        );
        // earlier rows scale by ratio / r_anchor
        assert!(
            (qfq[0].close - 25.11 * (1.0 / r_anchor)).abs() < 1e-9,
            "qfq first-day close must scale by 1.0 / r_anchor (got {})",
            qfq[0].close
        );
        assert!(
            (qfq[1].close - 27.0 * (2.0 / r_anchor)).abs() < 1e-9,
            "qfq second-day close must scale by 2.0 / r_anchor (got {})",
            qfq[1].close
        );
        // open/high/low scale by the same factor as close (per bar)
        assert!(
            (qfq[0].open - 24.0 * (1.0 / r_anchor)).abs() < 1e-9,
            "qfq day-1 open"
        );
        assert!(
            (qfq[0].high - 25.5 * (1.0 / r_anchor)).abs() < 1e-9,
            "qfq day-1 high"
        );
        assert!(
            (qfq[0].low - 23.5 * (1.0 / r_anchor)).abs() < 1e-9,
            "qfq day-1 low"
        );
        assert!(
            (qfq[1].open - 26.0 * (2.0 / r_anchor)).abs() < 1e-9,
            "qfq day-2 open"
        );
        assert!(
            (qfq[1].high - 28.0 * (2.0 / r_anchor)).abs() < 1e-9,
            "qfq day-2 high"
        );
        assert!(
            (qfq[1].low - 25.5 * (2.0 / r_anchor)).abs() < 1e-9,
            "qfq day-2 low"
        );
        // anchor bar itself: factor 1.0 → unchanged for all OHLC components
        assert!(
            (qfq[2].open - 25.0).abs() < 1e-9,
            "qfq anchor open unchanged"
        );
        assert!(
            (qfq[2].high - 26.0).abs() < 1e-9,
            "qfq anchor high unchanged"
        );
        assert!((qfq[2].low - 24.8).abs() < 1e-9, "qfq anchor low unchanged");

        // --- hfq: close == raw close × ratio (adjclose 口径) ---
        // first day ratio = 1.0 → untouched values.
        assert!(
            (hfq[0].close - 25.11).abs() < 1e-9,
            "hfq first-day close must stay at raw close (ratio 1.0)"
        );
        assert!(
            (hfq[0].open - 24.0).abs() < 1e-9,
            "hfq first-day open unchanged"
        );
        assert!(
            (hfq[1].close - 54.0).abs() < 1e-9,
            "hfq second-day close must equal its adjclose (54.0)"
        );
        assert!(
            (hfq[1].open - 52.0).abs() < 1e-9,
            "hfq second-day open = 26 × 2.0"
        );
        assert!(
            (hfq[2].close - 150.97).abs() < 1e-9,
            "hfq last close must equal its adjclose (150.97)"
        );
        assert!(
            (hfq[2].open - 25.0 * r_anchor).abs() < 1e-9,
            "hfq last open = raw open × ratio (≈150.31, got {})",
            hfq[2].open
        );

        // --- none: raw OHLC untouched ---
        assert_eq!(none[0].open, 24.0);
        assert_eq!(none[0].high, 25.5);
        assert_eq!(none[0].low, 23.5);
        assert_eq!(none[0].close, 25.11);
        assert_eq!(none[1].close, 27.0);
        assert_eq!(none[2].close, 25.11);

        // --- volume never scaled (all three modes, every row) ---
        let expected_volumes = [1000.0, 2000.0, 3000.0];
        for (bars, mode) in [(&qfq, "qfq"), (&hfq, "hfq"), (&none, "none")] {
            for (i, b) in bars.iter().enumerate() {
                assert_eq!(
                    b.volume, expected_volumes[i],
                    "{mode} row {i} volume must never scale"
                );
            }
        }

        // --- the three modes must be mutually distinguishable ---
        assert!((qfq[1].close - none[1].close).abs() > 1e-3, "qfq ≠ none");
        assert!((hfq[1].close - none[1].close).abs() > 1e-3, "hfq ≠ none");
        assert!((qfq[1].close - hfq[1].close).abs() > 1e-3, "qfq ≠ hfq");
    }

    /// Given daily rows spanning two calendar months (June: 3 rows, ratios
    /// 1.0/2.0/6.0; July: 1 row, ratio 7.0 → qfq anchor), the 1M aggregate
    /// must scale BEFORE aggregating (plan §1.5) and the last monthly bar
    /// must stay at the raw close of its last valid daily row.
    ///
    /// Discrimination: June high = MAX(12/7, 30/7, 12.0) = 12.0 under
    /// scale-then-aggregate; an aggregate-then-scale implementation would
    /// yield MAX(12, 15, 14) × 6/7 ≈ 12.857 — caught here. (The raw high is
    /// on the middle row, whose factor is NOT the largest.)
    #[tokio::test]
    async fn fetch_bars_monthly_qfq_scales_before_aggregate_and_keeps_last_month_raw() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        {
            let conn = provider.conn.lock().expect("mutex lock");
            insert_daily_rows(
                &conn,
                "000001",
                &[
                    // 2026-06-03 — June, ratio 1.0
                    ("2026-06-03", 10.0, 12.0, 9.0, 10.0, Some(10.0), 100.0),
                    // 2026-06-05 — June, ratio 2.0 (owns the raw MAX high)
                    ("2026-06-05", 11.0, 15.0, 10.0, 12.0, Some(24.0), 200.0),
                    // 2026-06-26 — June, ratio 6.0 (last June row)
                    ("2026-06-26", 13.0, 14.0, 12.0, 14.0, Some(84.0), 300.0),
                    // 2026-07-02 — July, ratio 7.0 → qfq r_anchor
                    ("2026-07-02", 15.0, 16.0, 14.0, 16.0, Some(112.0), 400.0),
                ],
            );
        }

        let qfq = provider
            .fetch_bars("000001", "1M", fetch_all_start(), fetch_all_end(), "qfq")
            .await
            .expect("qfq monthly fetch");

        assert_eq!(qfq.len(), 2, "June + July → two monthly bars");

        // June: scale-then-aggregate with factors [1/7, 2/7, 6/7].
        let june = &qfq[0];
        assert!(
            (june.open - 10.0 / 7.0).abs() < 1e-9,
            "June open = FIRST scaled (10 × 1/7)"
        );
        assert!(
            (june.high - 12.0).abs() < 1e-9,
            "June high must be MAX(12/7, 30/7, 12.0) = 12.0 (scale-then-aggregate); \
             an aggregate-then-scale path would yield ≈12.857"
        );
        assert!(
            (june.low - 9.0 / 7.0).abs() < 1e-9,
            "June low = MIN of scaled lows (9 × 1/7)"
        );
        assert!(
            (june.close - 12.0).abs() < 1e-9,
            "June close = LAST scaled (14 × 6/7)"
        );
        assert_eq!(june.volume, 600.0, "June volume = SUM (never scaled)");

        // July (anchor row): factor 1.0 → raw close = current price.
        let july = &qfq[1];
        assert!(
            (july.close - 16.0).abs() < 1e-9,
            "last monthly bar close must stay at the raw close (anchor)"
        );
        assert!(
            (july.open - 15.0).abs() < 1e-9,
            "July open unchanged (factor 1.0)"
        );
        assert_eq!(july.volume, 400.0, "July volume = SUM (never scaled)");

        // Cross-mode sanity on the same aggregate path: hfq/none must not
        // collapse to qfq (plan §4: 三档互异).
        let hfq = provider
            .fetch_bars("000001", "1M", fetch_all_start(), fetch_all_end(), "hfq")
            .await
            .expect("hfq monthly fetch");
        let none = provider
            .fetch_bars("000001", "1M", fetch_all_start(), fetch_all_end(), "none")
            .await
            .expect("none monthly fetch");
        assert_eq!(hfq.len(), 2);
        assert_eq!(none.len(), 2);
        assert!(
            (hfq[0].close - 84.0).abs() < 1e-9,
            "hfq June close = adjclose of the last June row (84.0)"
        );
        assert!(
            (none[0].close - 14.0).abs() < 1e-9,
            "none June close = raw last close (14.0)"
        );
    }
}
