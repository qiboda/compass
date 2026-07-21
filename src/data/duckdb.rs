use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use duckdb::{Connection, OptionalExt, params};
use egui_charts::model::Bar;

use crate::data::provider::{DataError, DataProvider, DataWriter, NegativeCache};
use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// Type-safe record structs for all 7 tables
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DailyRecord {
    pub trade_date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub change: f64,
    pub pct_chg: f64,
    pub vol: f64,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdjFactorRecord {
    pub trade_date: NaiveDate,
    pub adj_factor: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusRecord {
    pub trade_date: NaiveDate,
    pub is_open: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LimitRecord {
    pub trade_date: NaiveDate,
    pub up_limit: f64,
    pub down_limit: f64,
}

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
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open        DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    close       DOUBLE,
    pre_close   DOUBLE,
    change      DOUBLE,
    pct_chg     DOUBLE,
    vol         DOUBLE,
    amount      DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_adj_factor (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    adj_factor  DOUBLE NOT NULL,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_basic (
    ts_code     VARCHAR PRIMARY KEY,
    symbol      VARCHAR,
    name        VARCHAR,
    area        VARCHAR,
    industry    VARCHAR,
    market      VARCHAR,
    exchange    VARCHAR,
    list_date   DATE,
    delist_date DATE
);

CREATE TABLE IF NOT EXISTS stock_status (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    is_open     BOOLEAN DEFAULT TRUE,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_limit (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    up_limit    DOUBLE,
    down_limit  DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE TABLE IF NOT EXISTS daily_indicator (
    ts_code         VARCHAR NOT NULL,
    trade_date      DATE NOT NULL,
    turnover_rate   DOUBLE,
    turnover_rate_f DOUBLE,
    volume_ratio    DOUBLE,
    pe              DOUBLE,
    pe_ttm          DOUBLE,
    pb              DOUBLE,
    ps              DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE TABLE IF NOT EXISTS stock_share (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    total_share DOUBLE,
    float_share DOUBLE,
    free_share  DOUBLE,
    total_mv    DOUBLE,
    circ_mv     DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_date ON stock_daily(trade_date);
CREATE INDEX IF NOT EXISTS idx_adj_date ON stock_adj_factor(trade_date);
CREATE INDEX IF NOT EXISTS idx_status_date ON stock_status(trade_date);
CREATE INDEX IF NOT EXISTS idx_limit_date ON stock_limit(trade_date);
CREATE INDEX IF NOT EXISTS idx_indicator_date ON daily_indicator(trade_date);
CREATE INDEX IF NOT EXISTS idx_share_date ON stock_share(trade_date);

CREATE TABLE IF NOT EXISTS no_data_marks (
    symbol       TEXT NOT NULL,
    timeframe    TEXT NOT NULL,
    last_checked BIGINT NOT NULL,
    PRIMARY KEY (symbol, timeframe)
);
";

// ---------------------------------------------------------------------------
// DuckDbProvider — local persistent cache
// ---------------------------------------------------------------------------

pub struct DuckDbProvider {
    pub conn: Arc<Mutex<Connection>>,
}

impl DuckDbProvider {
    /// Open (or create) the DuckDB database at `path` and ensure the schema exists.
    pub fn new(path: &str) -> Result<Self, DataError> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory().map_err(DataError::Database)?
        } else {
            Connection::open(path).map_err(DataError::Database)?
        };
        conn.execute_batch(SCHEMA_SQL)
            .map_err(DataError::Database)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Convenience constructor for tests — opens an in-memory database.
    pub fn new_in_memory() -> Result<Self, DataError> {
        Self::new(":memory:")
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
        let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let naive_dt = naive.and_hms_opt(0, 0, 0)?;
        Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc))
    }

    // -----------------------------------------------------------------------
    // Direct table methods (non-trait) — for use by CLI downloader
    // -----------------------------------------------------------------------

    /// Return the MIN and MAX `trade_date` for a given `ts_code`, or `None` if
    /// no data exists.
    pub async fn get_stored_range(
        &self,
        ts_code: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        let ts_code = ts_code.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "SELECT CAST(MIN(trade_date) AS VARCHAR), CAST(MAX(trade_date) AS VARCHAR) FROM stock_daily WHERE ts_code = ?",
                )
                .map_err(DataError::Database)?;

            let result: Option<(Option<String>, Option<String>)> = stmt
                .query_row(params![ts_code], |row| {
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
    /// record in the batch).  Uses `INSERT OR REPLACE` to be idempotent.
    pub async fn save_stock_daily(
        &self,
        ts_code: &str,
        records: &[DailyRecord],
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        // Sort by trade_date ascending and compute pre_close = previous close.
        let mut sorted: Vec<&DailyRecord> = records.iter().collect();
        sorted.sort_by_key(|r| r.trade_date);

        let ts_code = ts_code.to_string();
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, f64, f64, f64, f64, Option<f64>, f64, f64, f64, f64)> = {
            let mut prev_close: Option<f64> = None;
            sorted
                .iter()
                .map(|r| {
                    let pre_close = prev_close;
                    prev_close = Some(r.close);
                    (
                        r.trade_date.format("%Y-%m-%d").to_string(),
                        r.open,
                        r.high,
                        r.low,
                        r.close,
                        pre_close,
                        r.change,
                        r.pct_chg,
                        r.vol,
                        r.amount,
                    )
                })
                .collect()
        };

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_daily
                        (ts_code, trade_date, open, high, low, close, pre_close, change, pct_chg, vol, amount)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, open, high, low, close, pre_close, change, pct_chg, vol, amount) in &rows {
                stmt.execute(params![
                    ts_code,
                    date_str,
                    open,
                    high,
                    low,
                    close,
                    pre_close,
                    change,
                    pct_chg,
                    vol,
                    amount,
                ])
                .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save adj_factor records into `stock_adj_factor`.
    pub async fn save_adj_factors(
        &self,
        ts_code: &str,
        factors: &[AdjFactorRecord],
    ) -> Result<(), DataError> {
        if factors.is_empty() {
            return Ok(());
        }

        let ts_code = ts_code.to_string();
        let owned: Vec<(String, f64)> = factors
            .iter()
            .map(|r| (r.trade_date.format("%Y-%m-%d").to_string(), r.adj_factor))
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_adj_factor (ts_code, trade_date, adj_factor) VALUES (?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, factor) in &owned {
                stmt.execute(params![ts_code, date_str, factor])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Return the MIN and MAX trade_date for adj_factor of a `ts_code`.
    pub async fn get_adj_factor_range(
        &self,
        ts_code: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        let ts_code = ts_code.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "SELECT CAST(MIN(trade_date) AS VARCHAR), CAST(MAX(trade_date) AS VARCHAR) FROM stock_adj_factor WHERE ts_code = ?",
                )
                .map_err(DataError::Database)?;

            let result: Option<(Option<String>, Option<String>)> = stmt
                .query_row(params![ts_code], |row| {
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
    pub async fn upsert_stock_basic(&self, info: &StockBasic) -> Result<(), DataError> {
        let ts_code = info.ts_code.clone();
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
            conn.execute(
                "INSERT OR REPLACE INTO stock_basic (ts_code, symbol, name, area, industry, market, exchange, list_date, delist_date)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![ts_code, symbol, name, area, industry, market, exchange, list_date_str, delist_date_str],
            )
            .map_err(DataError::Database)?;
            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Read a single stock_basic record.
    pub async fn get_stock_basic(&self, ts_code: &str) -> Result<Option<StockBasic>, DataError> {
        let ts_code = ts_code.to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare("SELECT ts_code, symbol, name, area, industry, market, exchange, CAST(list_date AS VARCHAR), CAST(delist_date AS VARCHAR) FROM stock_basic WHERE ts_code = ?")
                .map_err(DataError::Database)?;

            let result = stmt
                .query_row(params![ts_code], |row| {
                    Ok(StockBasic {
                        ts_code: row.get(0)?,
                        symbol: row.get(1)?,
                        name: row.get(2)?,
                        area: row.get::<_, Option<String>>(3)?,
                        industry: row.get::<_, Option<String>>(4)?,
                        market: row.get::<_, Option<String>>(5)?,
                        exchange: row.get::<_, Option<String>>(6)?,
                        list_date: row
                            .get::<_, Option<String>>(7)?
                            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                        delist_date: row
                            .get::<_, Option<String>>(8)?
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

    /// Save status records into `stock_status`.
    pub async fn save_status(
        &self,
        ts_code: &str,
        records: &[StatusRecord],
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let ts_code = ts_code.to_string();
        let owned: Vec<(String, bool)> = records
            .iter()
            .map(|r| (r.trade_date.format("%Y-%m-%d").to_string(), r.is_open))
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_status (ts_code, trade_date, is_open) VALUES (?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, is_open) in &owned {
                stmt.execute(params![ts_code, date_str, is_open])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save limit records into `stock_limit`.
    pub async fn save_limits(
        &self,
        ts_code: &str,
        records: &[LimitRecord],
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let ts_code = ts_code.to_string();
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

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_limit (ts_code, trade_date, up_limit, down_limit) VALUES (?, ?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, up_limit, down_limit) in &owned {
                stmt.execute(params![ts_code, date_str, up_limit, down_limit])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save indicator records into `daily_indicator`.
    pub async fn save_indicators(
        &self,
        ts_code: &str,
        records: &[IndicatorRecord],
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let ts_code = ts_code.to_string();
        #[allow(clippy::type_complexity)]
        let owned: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = records
            .iter()
            .map(|r| {
                (
                    r.trade_date.format("%Y-%m-%d").to_string(),
                    r.turnover_rate,
                    r.turnover_rate_f,
                    r.volume_ratio,
                    r.pe,
                    r.pe_ttm,
                    r.pb,
                    r.ps,
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO daily_indicator
                        (ts_code, trade_date, turnover_rate, turnover_rate_f, volume_ratio, pe, pe_ttm, pb, ps)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, tr, trf, vr, pe, pe_ttm, pb, ps) in &owned {
                stmt.execute(params![ts_code, date_str, tr, trf, vr, pe, pe_ttm, pb, ps])
                    .map_err(DataError::Database)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    /// Save share records into `stock_share`.
    pub async fn save_shares(
        &self,
        ts_code: &str,
        records: &[ShareRecord],
    ) -> Result<(), DataError> {
        if records.is_empty() {
            return Ok(());
        }

        let ts_code = ts_code.to_string();
        let owned: Vec<(String, f64, f64, f64, f64, f64)> = records
            .iter()
            .map(|r| {
                (
                    r.trade_date.format("%Y-%m-%d").to_string(),
                    r.total_share,
                    r.float_share,
                    r.free_share,
                    r.total_mv,
                    r.circ_mv,
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_share
                        (ts_code, trade_date, total_share, float_share, free_share, total_mv, circ_mv)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, ts, fs, free_s, tmv, cmv) in &owned {
                stmt.execute(params![ts_code, date_str, ts, fs, free_s, tmv, cmv])
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

#[derive(Debug, Clone)]
pub struct StockBasic {
    pub ts_code: String,
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
        _timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        let ts_code = crate::data::symbol::to_ts_code(symbol);
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "SELECT CAST(trade_date AS VARCHAR), open, high, low, close, vol
                     FROM stock_daily
                     WHERE ts_code = ? AND trade_date >= ? AND trade_date <= ?
                     ORDER BY trade_date ASC",
                )
                .map_err(DataError::Database)?;

            let rows: Vec<(String, f64, f64, f64, f64, f64)> = stmt
                .query_map(params![ts_code, start_str, end_str], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                })
                .map_err(DataError::Database)?
                .collect::<Result<Vec<_>, duckdb::Error>>()
                .map_err(DataError::Database)?;

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
        // Symbol search is handled by the remote provider (e.g. EastMoney).
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
    ) -> Result<(), DataError> {
        if bars.is_empty() {
            return Ok(());
        }

        let ts_code = crate::data::symbol::to_ts_code(symbol);
        let records: Vec<(String, f64, f64, f64, f64, f64)> = bars
            .iter()
            .map(|b| {
                (
                    b.time.format("%Y-%m-%d").to_string(),
                    b.open,
                    b.high,
                    b.low,
                    b.close,
                    b.volume,
                )
            })
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO stock_daily
                        (ts_code, trade_date, open, high, low, close, vol)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(DataError::Database)?;

            for (date_str, open, high, low, close, volume) in &records {
                stmt.execute(params![ts_code, date_str, open, high, low, close, volume])
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
        Bar {
            time: Utc::now()
                .date_naive()
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
    #[rstest]
    #[case("000001", "1d")]
    #[case("600519", "1w")]
    #[case("AAPL", "1M")]
    #[tokio::test]
    async fn save_and_fetch_preserves_symbol_and_timeframe(
        #[case] symbol: &str,
        #[case] timeframe: &str,
    ) {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let bars = vec![
            make_bar(1, 10.0, 10.5, 1000.0),
            make_bar(2, 10.5, 11.0, 2000.0),
        ];

        provider
            .save_bars(symbol, timeframe, &bars)
            .await
            .expect("save_bars failed");

        let fetched = provider
            .fetch_bars(symbol, timeframe, fetch_all_start(), fetch_all_end())
            .await
            .expect("fetch_bars failed");

        assert_eq!(fetched.len(), 2, "wrong count for {symbol}/{timeframe}");
        assert_eq!(fetched[0].open, 10.0);
        assert_eq!(fetched[0].close, 10.5);
        assert_eq!(fetched[1].open, 10.5);
        assert_eq!(fetched[1].close, 11.0);

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
        let conn = provider.conn.lock().expect("mutex lock");
        conn.execute(
            "INSERT INTO no_data_marks (symbol, timeframe, last_checked) VALUES (?, ?, ?)",
            params!["000003", "1d", stale_ts],
        )
        .expect("insert stale mark");
        drop(conn);

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
        let conn = provider.conn.lock().expect("mutex lock");
        let mut stmt = conn
            .prepare("INSERT INTO stock_daily (ts_code, trade_date, open, high, low, close, vol) VALUES (?, ?, 1, 2, 1, 2, 100)")
            .expect("prepare");
        stmt.execute(params!["000001.SZ", d2.format("%Y-%m-%d").to_string()])
            .expect("insert d2");
        stmt.execute(params!["000001.SZ", d1.format("%Y-%m-%d").to_string()])
            .expect("insert d1");
        stmt.execute(params!["000001.SZ", d3.format("%Y-%m-%d").to_string()])
            .expect("insert d3");
        drop(stmt);
        drop(conn);

        let range = provider
            .get_stored_range("000001.SZ")
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

        let records = vec![
            DailyRecord {
                trade_date: d1,
                open: 15.0,
                high: 16.0,
                low: 14.5,
                close: 15.5,
                change: std::f64::NAN,
                pct_chg: std::f64::NAN,
                vol: 1000.0,
                amount: 15000.0,
            },
            DailyRecord {
                trade_date: d2,
                open: 15.5,
                high: 17.0,
                low: 15.0,
                close: 16.5,
                change: 1.0,
                pct_chg: 6.45,
                vol: 2000.0,
                amount: 33000.0,
            },
        ];

        provider
            .save_stock_daily("000001.SZ", &records)
            .await
            .expect("save_stock_daily failed");

        let range = provider
            .get_stored_range("000001.SZ")
            .await
            .expect("get_stored_range failed");
        assert_eq!(range, Some((d1, d2)));
    }

    #[tokio::test]
    async fn save_stock_daily_computes_pre_close_from_previous_close() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 6, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 6, 2).expect("valid date");
        let d3 = NaiveDate::from_ymd_opt(2025, 6, 3).expect("valid date");

        // Insert out of order — save_stock_daily sorts by trade_date.
        let records = vec![
            DailyRecord {
                trade_date: d2,
                open: 21.0,
                high: 22.0,
                low: 20.5,
                close: 21.5,
                change: 0.5,
                pct_chg: 2.38,
                vol: 500.0,
                amount: 10750.0,
            },
            DailyRecord {
                trade_date: d1,
                open: 20.0,
                high: 21.0,
                low: 19.5,
                close: 21.0,
                change: 1.0,
                pct_chg: 5.0,
                vol: 300.0,
                amount: 6300.0,
            },
            DailyRecord {
                trade_date: d3,
                open: 21.5,
                high: 23.0,
                low: 21.0,
                close: 22.0,
                change: 0.5,
                pct_chg: 2.33,
                vol: 800.0,
                amount: 17600.0,
            },
        ];

        provider
            .save_stock_daily("000001.SZ", &records)
            .await
            .expect("save_stock_daily failed");

        // Verify pre_close directly
        let conn = provider.conn.lock().expect("mutex lock");
        let mut stmt = conn
            .prepare(
                "SELECT CAST(trade_date AS VARCHAR), pre_close FROM stock_daily WHERE ts_code = '000001.SZ' ORDER BY trade_date ASC",
            )
            .expect("prepare");
        let rows: Vec<(String, Option<f64>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query_map")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        drop(stmt);
        drop(conn);

        assert_eq!(rows.len(), 3, "expected 3 rows");

        // d1 (first record): pre_close should be NULL
        assert_eq!(rows[0].0, d1.format("%Y-%m-%d").to_string());
        assert!(rows[0].1.is_none(), "first record pre_close should be NULL");

        // d2: pre_close = d1's close (21.0)
        assert_eq!(rows[1].0, d2.format("%Y-%m-%d").to_string());
        assert!(
            (rows[1].1.unwrap() - 21.0).abs() < 0.001,
            "d2 pre_close should be 21.0"
        );

        // d3: pre_close = d2's close (21.5)
        assert_eq!(rows[2].0, d3.format("%Y-%m-%d").to_string());
        assert!(
            (rows[2].1.unwrap() - 21.5).abs() < 0.001,
            "d3 pre_close should be 21.5"
        );
    }

    #[tokio::test]
    async fn save_stock_daily_empty_records_does_nothing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        provider
            .save_stock_daily("000001.SZ", &[])
            .await
            .expect("save_stock_daily with empty records failed");
        let range = provider
            .get_stored_range("000001.SZ")
            .await
            .expect("get_stored_range failed");
        assert!(range.is_none());
    }

    // -----------------------------------------------------------------------
    // stock_basic tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn upsert_and_get_stock_basic() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let info = StockBasic {
            ts_code: "000001.SZ".into(),
            symbol: "平安银行".into(),
            name: "平安银行股份有限公司".into(),
            area: Some("深圳".into()),
            industry: Some("银行".into()),
            market: Some("主板".into()),
            exchange: Some("SZ".into()),
            list_date: NaiveDate::from_ymd_opt(1991, 4, 3),
            delist_date: None,
        };

        provider
            .upsert_stock_basic(&info)
            .await
            .expect("upsert_stock_basic failed");

        let fetched = provider
            .get_stock_basic("000001.SZ")
            .await
            .expect("get_stock_basic failed")
            .expect("should have data");

        assert_eq!(fetched.ts_code, "000001.SZ");
        assert_eq!(fetched.symbol, "平安银行");
        assert_eq!(fetched.name, "平安银行股份有限公司");
        assert_eq!(fetched.area.as_deref(), Some("深圳"));
        assert_eq!(fetched.industry.as_deref(), Some("银行"));
        assert_eq!(fetched.list_date, info.list_date);
    }

    #[tokio::test]
    async fn get_stock_basic_returns_none_for_unknown() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let info = provider
            .get_stock_basic("NOTEXIST.SZ")
            .await
            .expect("get_stock_basic failed");
        assert!(info.is_none());
    }

    // -----------------------------------------------------------------------
    // stock_status tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_status_inserts_and_verifies() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d = NaiveDate::from_ymd_opt(2025, 7, 1).expect("valid date");
        provider
            .save_status(
                "000001.SZ",
                &[StatusRecord {
                    trade_date: d,
                    is_open: true,
                }],
            )
            .await
            .expect("save_status failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_status WHERE ts_code = '000001.SZ' AND is_open = TRUE",
                [],
                |row| row.get(0),
            )
            .expect("query");
        drop(conn);
        assert_eq!(count, 1);
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
                "000001.SZ",
                &[LimitRecord {
                    trade_date: d,
                    up_limit: 16.5,
                    down_limit: 13.5,
                }],
            )
            .await
            .expect("save_limits failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let (up, down): (f64, f64) = conn
            .query_row(
                "SELECT up_limit, down_limit FROM stock_limit WHERE ts_code = '000001.SZ'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        drop(conn);

        assert!((up - 16.5).abs() < 0.001);
        assert!((down - 13.5).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // daily_indicator tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_indicators_inserts_and_reads() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d = NaiveDate::from_ymd_opt(2025, 7, 1).expect("valid date");
        provider
            .save_indicators(
                "000001.SZ",
                &[IndicatorRecord {
                    trade_date: d,
                    turnover_rate: 0.5,
                    turnover_rate_f: 0.3,
                    volume_ratio: 1.2,
                    pe: 5.0,
                    pe_ttm: 4.8,
                    pb: 0.8,
                    ps: 1.5,
                }],
            )
            .await
            .expect("save_indicators failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let (pe, pb): (f64, f64) = conn
            .query_row(
                "SELECT pe, pb FROM daily_indicator WHERE ts_code = '000001.SZ'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        drop(conn);

        assert!((pe - 5.0).abs() < 0.001);
        assert!((pb - 0.8).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // stock_share tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_shares_inserts_and_reads() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d = NaiveDate::from_ymd_opt(2025, 7, 1).expect("valid date");
        provider
            .save_shares(
                "000001.SZ",
                &[ShareRecord {
                    trade_date: d,
                    total_share: 194.06,
                    float_share: 194.06,
                    free_share: 135.84,
                    total_mv: 2910.9,
                    circ_mv: 2910.9,
                }],
            )
            .await
            .expect("save_shares failed");

        let conn = provider.conn.lock().expect("mutex lock");
        let (total_share, total_mv): (f64, f64) = conn
            .query_row(
                "SELECT total_share, total_mv FROM stock_share WHERE ts_code = '000001.SZ'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        drop(conn);

        assert!((total_share - 194.06).abs() < 0.001);
        assert!((total_mv - 2910.9).abs() < 0.001);
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
                "000001.SZ",
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
            )
            .await
            .expect("save_adj_factors failed");

        let range = provider
            .get_adj_factor_range("000001.SZ")
            .await
            .expect("get_adj_factor_range failed");
        assert_eq!(range, Some((d1, d2)));
    }

    #[tokio::test]
    async fn get_adj_factor_range_returns_none_when_empty() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let range = provider
            .get_adj_factor_range("000001.SZ")
            .await
            .expect("get_adj_factor_range failed");
        assert!(range.is_none());
    }
}
