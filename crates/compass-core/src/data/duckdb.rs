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

CREATE TABLE IF NOT EXISTS stock_basic (
    symbol      VARCHAR PRIMARY KEY,
    name        VARCHAR,
    area        VARCHAR,
    industry    VARCHAR,
    market      VARCHAR,
    exchange    VARCHAR,
    list_date   DATE,
    delist_date DATE
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

    /// Upsert a stock_basic record.
    ///
    /// When `overwrite` is false, an existing row with the same symbol is skipped.
    /// When true, the existing row is replaced.
    pub async fn upsert_stock_basic(
        &self,
        info: &StockBasic,
        overwrite: bool,
    ) -> Result<(), DataError> {
        let symbol = info.symbol.clone();
        let name = info.name.clone();
        let area = info.area.clone();
        let industry = info.industry.clone();
        let market = info.market.clone();
        let exchange = info.exchange.clone();
        let list_date_str = info.list_date.map(|d| d.format("%Y-%m-%d").to_string());
        let delist_date_str = info.delist_date.map(|d| d.format("%Y-%m-%d").to_string());

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let verb = if overwrite {
                "INSERT OR REPLACE"
            } else {
                "INSERT OR IGNORE"
            };
            let sql = format!(
                "{verb} INTO stock_basic (symbol, name, area, industry, market, exchange, list_date, delist_date)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            );
            conn.execute(&sql,
                params![symbol, name, area, industry, market, exchange, list_date_str, delist_date_str],
            )
            .map_err(DataError::Database)?;
            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Read a single stock_basic record.
    pub async fn get_stock_basic(&self, symbol: &str) -> Result<Option<StockBasic>, DataError> {
        let symbol = symbol.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare("SELECT symbol, name, area, industry, market, exchange, CAST(list_date AS VARCHAR), CAST(delist_date AS VARCHAR) FROM stock_basic WHERE symbol = ?")
                .map_err(DataError::Database)?;

            let result = stmt
                .query_row(params![symbol], |row| {
                    Ok(StockBasic {
                        symbol: row.get(0)?,
                        name: row.get(1)?,
                        area: row.get::<_, Option<String>>(2)?,
                        industry: row.get::<_, Option<String>>(3)?,
                        market: row.get::<_, Option<String>>(4)?,
                        exchange: row.get::<_, Option<String>>(5)?,
                        list_date: row
                            .get::<_, Option<String>>(6)?
                            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                        delist_date: row
                            .get::<_, Option<String>>(7)?
                            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                    })
                })
                .optional()
                .map_err(DataError::Database)?;

            Ok(result)
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
// StockBasic — read-back struct for stock_basic table
// ---------------------------------------------------------------------------

/// Read-back struct for the `stock_basic` table.
#[derive(Debug, Clone)]
pub struct StockBasic {
    pub symbol: String,
    pub name: String,
    pub area: Option<String>,
    pub industry: Option<String>,
    pub market: Option<String>,
    pub exchange: Option<String>,
    pub list_date: Option<NaiveDate>,
    pub delist_date: Option<NaiveDate>,
}

// ---------------------------------------------------------------------------
// DataProvider — read-only data source
// ---------------------------------------------------------------------------

#[async_trait]
impl DataProvider for DuckDbProvider {
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        let symbol = symbol.to_string();
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();
        let conn = Arc::clone(&self.conn);
        let parquet_dir = self.parquet_dir.clone();
        let timeframe = timeframe.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "SELECT CAST(trade_date AS VARCHAR), open, high, low, close, volume
                     FROM stock_daily
                     WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
                     ORDER BY trade_date ASC",
                )
                .map_err(DataError::Database)?;

            let mut rows: Vec<(String, f64, f64, f64, f64, f64)> = stmt
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

                    // Build 6-tuple rows for the normal flow (open, high, low, close, volume)
                    rows = parquet_rows
                        .iter()
                        .map(|(d, o, h, l, c, v, _, _)| {
                            (d.clone(), *o, *h, *l, *c, *v)
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

            // Issue #46: timeframe aggregation — re-query with date_trunc
            // GROUP BY for weekly/monthly OHLCV resample from daily data.
            if timeframe != "1d" && !rows.is_empty() {
                let unit = match timeframe.as_str() {
                    "1w" => "week",
                    "1M" => "month",
                    _ => "day",
                };
                let sql = format!(
                    "SELECT CAST(grp_date AS VARCHAR) as trade_date,
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
                             SELECT * FROM stock_daily
                             WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
                             ORDER BY trade_date ASC
                         )
                         GROUP BY grp_date
                     ) ORDER BY trade_date"
                );
                let mut agg_stmt = conn.prepare(&sql).map_err(DataError::Database)?;
                rows = agg_stmt
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
                            ))
                        },
                    )
                    .map_err(DataError::Database)?
                    .collect::<Result<Vec<_>, duckdb::Error>>()
                    .map_err(DataError::Database)?;
            }

            let bars: Vec<Bar> = rows
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

            Ok(bars)
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
            .fetch_bars(symbol, timeframe, fetch_all_start(), fetch_all_end())
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
            .fetch_bars("NOT_EXIST", timeframe, fetch_all_start(), fetch_all_end())
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
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
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
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .expect("fetch");

        assert_eq!(bars.len(), 1);
        assert!(
            (bars[0].close - 99.0).abs() < 0.01,
            "close should be 99.0 after overwrite"
        );
    }

    // -----------------------------------------------------------------------
    // stock_basic tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn upsert_and_get_stock_basic() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let info = StockBasic {
            symbol: "000001".into(),
            name: "平安银行".into(),
            area: Some("深圳".into()),
            industry: Some("银行".into()),
            market: Some("主板".into()),
            exchange: Some("SZ".into()),
            list_date: NaiveDate::from_ymd_opt(1991, 4, 3),
            delist_date: None,
        };

        provider
            .upsert_stock_basic(&info, true)
            .await
            .expect("upsert_stock_basic failed");

        let fetched = provider
            .get_stock_basic("000001")
            .await
            .expect("get_stock_basic failed")
            .expect("should have data");

        assert_eq!(fetched.symbol, "000001");
        assert_eq!(fetched.name, "平安银行");
        assert_eq!(fetched.area.as_deref(), Some("深圳"));
        assert_eq!(fetched.industry.as_deref(), Some("银行"));
        assert_eq!(fetched.list_date, info.list_date);
    }

    #[tokio::test]
    async fn get_stock_basic_returns_none_for_unknown() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let info = provider
            .get_stock_basic("999999")
            .await
            .expect("get_stock_basic failed");
        assert!(info.is_none());
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
            .fetch_bars("000001", "1d", start, end)
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
            .fetch_bars("000001", "1d", start, end)
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
            .fetch_bars("000001", "1d", start, end)
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
            .fetch_bars("'; DROP TABLE stock_daily; --", "1d", start, end)
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
            .fetch_bars("000001", "1d", start, end)
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
            .fetch_bars("000001", timeframe, start, end)
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
            .fetch_bars("000001", "1d", start, end)
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
    // upsert_stock_basic: overwrite=false
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn upsert_stock_basic_skips_existing_when_overwrite_false() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let original = StockBasic {
            symbol: "000001".into(),
            name: "OldName".into(),
            area: Some("Area1".into()),
            industry: Some("Industry1".into()),
            market: Some("Market1".into()),
            exchange: Some("EX1".into()),
            list_date: NaiveDate::from_ymd_opt(2020, 1, 1),
            delist_date: None,
        };
        let updated = StockBasic {
            symbol: "000001".into(),
            name: "NewName".into(),
            area: Some("Area2".into()),
            industry: Some("Industry2".into()),
            market: Some("Market2".into()),
            exchange: Some("EX2".into()),
            list_date: NaiveDate::from_ymd_opt(2021, 1, 1),
            delist_date: None,
        };

        provider
            .upsert_stock_basic(&original, true)
            .await
            .expect("first upsert");
        provider
            .upsert_stock_basic(&updated, false)
            .await
            .expect("second upsert with skip");

        let fetched = provider
            .get_stock_basic("000001")
            .await
            .expect("get_stock_basic failed")
            .expect("should exist");
        assert_eq!(fetched.name, "OldName");
        assert_eq!(fetched.area.as_deref(), Some("Area1"));
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
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
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
            .fetch_bars("000001", "1d", start, end)
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
            .fetch_bars("000001", "4h", fetch_all_start(), fetch_all_end())
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
            .fetch_bars("000001", "1d", start, end)
            .await
            .expect("fetch_bars should not error");
        assert!(bars.is_empty());
    }
}
