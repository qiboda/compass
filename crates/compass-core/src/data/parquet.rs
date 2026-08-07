//! Parquet-based main database reader.
//!
//! Queries a single `stock_daily.parquet` file directly via DuckDB's
//! `read_parquet()` function with `WHERE symbol = ?` filtering, without
//! loading data into DuckDB tables.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use duckdb::{Connection, OptionalExt, params};
use egui_charts::model::Bar;

use crate::data::provider::{DataError, DataProvider};
use crate::indicators::{RawBar, adjust_ohlc};
use crate::model::{
    BlockTradeRow, CapitalMainFlow, ConceptMember, CrossSectionBar, DragonListRow,
    InstitutionSurveyRow, StockBasic, SymbolInfo,
};

/// One row from the daily parquet query: (date_str, open, high, low, close,
/// volume, adjclose). `adjclose` is optional — NULL rows keep factor 1.0
/// during forward adjustment.
type DailyRow = (String, f64, f64, f64, f64, f64, Option<f64>);

/// Validate symbol for use in DuckDB parameter bindings.
///
/// With the single-file format, symbols are bound as DuckDB parameters (`?`),
/// not inserted into SQL strings. This function provides defense-in-depth:
/// only the canonical exchange-prefixed form (`SH`/`SZ`/`BJ` + 6 digits, e.g.
/// `SZ000001`) is accepted (D9). Bare codes, dot forms (`sh.000001`) and any
/// other alphanumeric shapes are rejected.
pub(crate) fn validate_symbol(symbol: &str) -> Result<&str, DataError> {
    let bytes = symbol.as_bytes();
    let valid = bytes.len() == 8
        && matches!(&bytes[..2], b"SH" | b"SZ" | b"BJ")
        && bytes[2..].iter().all(u8::is_ascii_digit);
    if !valid {
        return Err(DataError::NoData {
            symbol: symbol.to_string(),
        });
    }
    Ok(symbol)
}

/// Escape single quotes in path strings used inside `read_parquet('...')` SQL.
///
/// Paths derived from config files may contain quote characters that would
/// close the string literal. Doubling the quote is the standard SQL escape.
fn escape_sql_path(path: &str) -> String {
    path.replace('\'', "''")
}

/// Read A-share OHLCV data from a single Parquet file with a `symbol` column.
///
/// Expected directory layout:
/// ```text
/// parquet_data/
///   stock_basic.parquet
///   stock_daily.parquet
///   stock_daily.symbols.txt     (optional companion, one symbol per line)
/// ```
pub struct ParquetReader {
    conn: Arc<Mutex<Connection>>,
    parquet_dir: PathBuf,
    daily_path: PathBuf,
    basic_path: PathBuf,
}

impl ParquetReader {
    /// Create a new reader pointing at `parquet_dir`.
    ///
    /// The directory must contain `stock_basic.parquet` and a `stock_daily.parquet`
    /// file with a `symbol` column.
    pub fn new(parquet_dir: impl AsRef<Path>) -> Result<Self, DataError> {
        let dir = parquet_dir.as_ref();
        let conn = Connection::open_in_memory().map_err(DataError::Database)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            parquet_dir: dir.to_path_buf(),
            daily_path: dir.join("stock_daily.parquet"),
            basic_path: dir.join("stock_basic.parquet"),
        })
    }

    /// Run a query against a single Parquet table and map each row to `T`.
    ///
    /// Shared plumbing for the SEPA table read primitives: returns an empty
    /// vec when the file doesn't exist (table not yet imported), otherwise
    /// prepares `sql` on the in-memory DuckDB connection and collects mapped
    /// rows.
    fn query_parquet<T, F>(
        &self,
        path: &Path,
        sql: &str,
        params: &[&dyn duckdb::ToSql],
        map: F,
    ) -> Result<Vec<T>, DataError>
    where
        F: FnMut(&duckdb::Row) -> Result<T, duckdb::Error>,
    {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
        let mut stmt = conn.prepare(sql).map_err(DataError::Database)?;
        stmt.query_map(params, map)
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)
    }

    /// Fetch bars for a symbol and date range from the single Parquet file.
    /// Bars are **forward-adjusted** (前复权): OHLC is scaled by
    /// `factor_i = adjclose_i / close_i`, so the latest bar's price equals the
    /// current market price.
    pub fn fetch_bars_blocking(
        &self,
        symbol: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        validate_symbol(symbol)?;
        if !self.daily_path.exists() {
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        let path_str = self.daily_path.to_string_lossy();
        let escaped = escape_sql_path(&path_str);
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();

        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume, adjclose
             FROM read_parquet('{escaped}')
             WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
             ORDER BY tradedate ASC"
        );
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<DailyRow> = stmt
            .query_map(params![symbol, start_str, end_str], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)?;

        // Forward-adjust (ref #176): scale each bar by adjclose/close. Rows
        // with NULL adjclose keep factor 1.0 (no scaling).
        let mut raw: Vec<RawBar> = Vec::with_capacity(rows.len());
        let mut adjclose: Vec<f64> = Vec::with_capacity(rows.len());
        for (date_str, open, high, low, close, volume, adj) in rows {
            let Some(time) = date_str_to_utc(&date_str) else {
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
            adjclose.push(adj.unwrap_or(close));
        }

        let bars = adjust_ohlc(&raw, &adjclose);

        if bars.is_empty() {
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        Ok(bars)
    }

    /// List all available symbols.
    ///
    /// First tries `stock_daily.symbols.txt` (fast, one symbol per line).
    /// Falls back to `SELECT DISTINCT symbol FROM read_parquet(...)` if the
    /// text file is missing. Returns empty vec if neither source exists.
    pub fn list_symbols(&self) -> Result<Vec<SymbolInfo>, DataError> {
        let symbols_txt = self
            .daily_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("stock_daily.symbols.txt");

        // Fast path: read from companion text file
        if symbols_txt.exists() {
            match std::fs::read_to_string(&symbols_txt) {
                Ok(content) => {
                    let symbols: Vec<SymbolInfo> = content
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty())
                        .map(|code| SymbolInfo {
                            code: code.to_string(),
                            name: String::new(),
                        })
                        .collect();
                    return Ok(symbols);
                }
                Err(_) => {
                    // If read fails, fall through to SQL fallback
                }
            }
        }

        // Slow path: query the parquet file directly
        if !self.daily_path.exists() {
            return Ok(Vec::new());
        }

        let path_str = self.daily_path.to_string_lossy();
        let escaped = escape_sql_path(&path_str);
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!("SELECT DISTINCT symbol FROM read_parquet('{escaped}') ORDER BY symbol");
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)?;

        let symbols = rows
            .into_iter()
            .map(|code| SymbolInfo {
                code,
                name: String::new(),
            })
            .collect();
        Ok(symbols)
    }

    /// Get stored date range for a symbol from the single Parquet file.
    pub fn get_stored_range(
        &self,
        symbol: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        validate_symbol(symbol)?;
        if !self.daily_path.exists() {
            return Ok(None);
        }

        let path_str = self.daily_path.to_string_lossy().to_string();
        let escaped = escape_sql_path(&path_str);
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT CAST(MIN(tradedate) AS VARCHAR), CAST(MAX(tradedate) AS VARCHAR)
             FROM read_parquet('{escaped}')
             WHERE symbol = ?"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let result = stmt
            .query_row(params![symbol], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .optional()
            .map_err(DataError::Database)?;

        match result {
            Some((Some(min_s), Some(max_s))) if !min_s.is_empty() && !max_s.is_empty() => {
                let min_date = date_str_to_utc(&min_s)
                    .map(|dt| dt.date_naive())
                    .ok_or_else(|| DataError::Parse(format!("invalid date '{min_s}'")))?;
                let max_date = date_str_to_utc(&max_s)
                    .map(|dt| dt.date_naive())
                    .ok_or_else(|| DataError::Parse(format!("invalid date '{max_s}'")))?;
                Ok(Some((min_date, max_date)))
            }
            _ => Ok(None),
        }
    }

    /// Latest trade date present in `stock_daily.parquet`, if any.
    ///
    /// Used by the SEPA CLI as the default scoring date (decision 22: only
    /// the latest trading day, never the wall-clock date). Returns `None`
    /// when the file is missing or empty.
    pub fn latest_trade_date(&self) -> Result<Option<NaiveDate>, DataError> {
        if !self.daily_path.exists() {
            return Ok(None);
        }
        let path_str = self.daily_path.to_string_lossy();
        let escaped = escape_sql_path(&path_str);
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
        let sql = format!("SELECT CAST(MAX(tradedate) AS VARCHAR) FROM read_parquet('{escaped}')");
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let max_s: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .optional()
            .map_err(DataError::Database)?;
        match max_s {
            Some(s) if !s.is_empty() => {
                let dt = date_str_to_utc(&s)
                    .ok_or_else(|| DataError::Parse(format!("invalid date '{s}'")))?;
                Ok(Some(dt.date_naive()))
            }
            _ => Ok(None),
        }
    }

    /// Get stock basic info by reading stock_basic.parquet and filtering.
    pub fn get_stock_basic_blocking(&self, symbol: &str) -> Result<Option<StockBasic>, DataError> {
        validate_symbol(symbol)?;
        if !self.basic_path.exists() {
            return Ok(None);
        }

        let path_str = self.basic_path.to_string_lossy().to_string();
        let escaped = escape_sql_path(&path_str);
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT symbol, name, CAST(list_date AS VARCHAR) AS list_date, CAST(delist_date AS VARCHAR) AS delist_date,
                    board, full_name, CAST(total_share AS DOUBLE) AS total_share, industry, region
             FROM read_parquet('{escaped}')
             WHERE symbol = ?"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let result = stmt
            .query_row(params![symbol], |row| {
                Ok(StockBasic {
                    symbol: row.get("symbol")?,
                    name: row.get::<_, Option<String>>("name")?.unwrap_or_default(),
                    area: row.get::<_, Option<String>>("region")?,
                    industry: row.get::<_, Option<String>>("industry")?,
                    market: None,
                    board: row.get::<_, Option<String>>("board")?,
                    full_name: row.get::<_, Option<String>>("full_name")?,
                    total_share: row.get::<_, Option<f64>>("total_share")?,
                    list_date: row
                        .get::<_, Option<String>>("list_date")?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                    delist_date: row
                        .get::<_, Option<String>>("delist_date")?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                })
            })
            .map_err(|e| match e {
                duckdb::Error::QueryReturnedNoRows => DataError::NoData {
                    symbol: symbol.to_string(),
                },
                other => DataError::Database(other),
            })?;

        Ok(Some(result))
    }

    /// Load all rows from `stock_basic.parquet`.
    ///
    /// Returns the full stock list (symbol, name) for use in
    /// the GUI symbol picker. If `stock_basic.parquet` doesn't exist,
    /// returns an empty vec.
    pub fn load_all_stock_basics(&self) -> Result<Vec<StockBasic>, DataError> {
        if !self.basic_path.exists() {
            return Ok(Vec::new());
        }

        let path_str = self.basic_path.to_string_lossy().to_string();
        let escaped = escape_sql_path(&path_str);
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT symbol, name, CAST(list_date AS VARCHAR) AS list_date, CAST(delist_date AS VARCHAR) AS delist_date,
                    board, full_name, CAST(total_share AS DOUBLE) AS total_share, industry, region
             FROM read_parquet('{escaped}')
             ORDER BY symbol"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<StockBasic> = stmt
            .query_map([], |row| {
                Ok(StockBasic {
                    symbol: row.get("symbol")?,
                    name: row.get::<_, Option<String>>("name")?.unwrap_or_default(),
                    area: row.get::<_, Option<String>>("region")?,
                    industry: row.get::<_, Option<String>>("industry")?,
                    market: None,
                    board: row.get::<_, Option<String>>("board")?,
                    full_name: row.get::<_, Option<String>>("full_name")?,
                    total_share: row.get::<_, Option<f64>>("total_share")?,
                    list_date: row
                        .get::<_, Option<String>>("list_date")?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                    delist_date: row
                        .get::<_, Option<String>>("delist_date")?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                })
            })
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)?;

        Ok(rows)
    }

    /// Load all daily bars for every symbol in `stock_daily.parquet` within
    /// `[range_start, range_end]` (inclusive).
    ///
    /// This is the cross-section primitive used by whole-market scans
    /// (e.g. the screener). Unlike `fetch_bars_blocking` it filters by date
    /// only — no `WHERE symbol = ?` — so a single query returns bars for all
    /// symbols. If `stock_daily.parquet` doesn't exist, returns an empty vec.
    ///
    /// Dates are bound as `YYYY-MM-DD` strings, matching `fetch_bars_blocking`.
    /// The parquet `tradedate` column is `TIMESTAMP` (or `DATE`); `CAST AS
    /// VARCHAR` yields `"1991-04-04"` or `"1991-04-04 00:00:00"`, both parsed
    /// by `date_str_to_utc`.
    pub fn fetch_cross_section(
        &self,
        range_start: NaiveDate,
        range_end: NaiveDate,
    ) -> Result<Vec<CrossSectionBar>, DataError> {
        if !self.daily_path.exists() {
            return Ok(Vec::new());
        }

        let path_str = self.daily_path.to_string_lossy();
        let escaped = escape_sql_path(&path_str);
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();

        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT symbol, CAST(tradedate AS VARCHAR) AS tradedate, open, high, low, adjclose, close, volume, amount
             FROM read_parquet('{escaped}')
             WHERE tradedate >= ? AND tradedate <= ?
             ORDER BY symbol, tradedate ASC"
        );
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let bars: Vec<CrossSectionBar> = stmt
            .query_map(params![start_str, end_str], |row| {
                Ok(CrossSectionBar {
                    symbol: row.get("symbol")?,
                    trade_date: parse_naive_date(row.get("tradedate")?)?,
                    open: row.get("open")?,
                    high: row.get("high")?,
                    low: row.get("low")?,
                    adjclose: row.get("adjclose")?,
                    close: row.get("close")?,
                    volume: row.get("volume")?,
                    amount: row.get("amount")?,
                })
            })
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)?;

        Ok(bars)
    }

    /// Load all concept-board membership rows from `concept_member.parquet`.
    ///
    /// No date filter — the table is a versioned snapshot, not a time series
    /// (epic #139 decision 20). If the file doesn't exist, returns an empty vec.
    pub fn fetch_concept_member(&self) -> Result<Vec<ConceptMember>, DataError> {
        let path = self.parquet_dir.join("concept_member.parquet");
        let sql = format!(
            "SELECT concept_code, symbol, concept_name, CAST(update_date AS VARCHAR) AS update_date
             FROM read_parquet('{}')
             ORDER BY concept_code, symbol",
            escape_sql_path(&path.to_string_lossy())
        );
        self.query_parquet(&path, &sql, params![], |row| {
            Ok(ConceptMember {
                concept_code: row.get(0)?,
                symbol: row.get(1)?,
                concept_name: row.get(2)?,
                update_date: parse_naive_date_opt(row.get(3)?)?,
            })
        })
    }

    /// Load daily main-capital net flows within `[start, end]` (inclusive).
    ///
    /// Null amounts are coalesced to `0.0`; `small_net` stays `Option` because
    /// the source may omit it. If `capital_main_flow.parquet` doesn't exist,
    /// returns an empty vec.
    pub fn fetch_capital_main_flow(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<CapitalMainFlow>, DataError> {
        let path = self.parquet_dir.join("capital_main_flow.parquet");
        let sql = format!(
            "SELECT symbol, CAST(trade_date AS VARCHAR) AS trade_date,
                    COALESCE(main_net_inflow, 0.0), COALESCE(main_net_inflow_rate, 0.0),
                    COALESCE(super_large_net, 0.0), COALESCE(large_net, 0.0),
                    COALESCE(medium_net, 0.0), small_net,
                    CAST(update_date AS VARCHAR) AS update_date
             FROM read_parquet('{}')
             WHERE trade_date >= ? AND trade_date <= ?
             ORDER BY symbol, trade_date ASC",
            escape_sql_path(&path.to_string_lossy())
        );
        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();
        self.query_parquet(&path, &sql, params![start_str, end_str], |row| {
            Ok(CapitalMainFlow {
                symbol: row.get(0)?,
                trade_date: parse_naive_date(row.get(1)?)?,
                main_net_inflow: row.get(2)?,
                main_net_inflow_rate: row.get(3)?,
                super_large_net: row.get(4)?,
                large_net: row.get(5)?,
                medium_net: row.get(6)?,
                small_net: row.get(7)?,
                update_date: parse_naive_date_opt(row.get(8)?)?,
            })
        })
    }

    /// Load 龙虎榜 (dragon-tiger list) rows within `[start, end]` (inclusive).
    ///
    /// Null buy/sell amounts are coalesced to `0.0`. If `dragon_list.parquet`
    /// doesn't exist, returns an empty vec.
    pub fn fetch_dragon_list(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<DragonListRow>, DataError> {
        let path = self.parquet_dir.join("dragon_list.parquet");
        let sql = format!(
            "SELECT symbol, CAST(trade_date AS VARCHAR) AS trade_date, seat_type,
                    COALESCE(buy_amount, 0.0), COALESCE(sell_amount, 0.0),
                    net_amount, institution_flag,
                    CAST(update_date AS VARCHAR) AS update_date
             FROM read_parquet('{}')
             WHERE trade_date >= ? AND trade_date <= ?
             ORDER BY symbol, trade_date ASC, seat_type ASC",
            escape_sql_path(&path.to_string_lossy())
        );
        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();
        self.query_parquet(&path, &sql, params![start_str, end_str], |row| {
            Ok(DragonListRow {
                symbol: row.get(0)?,
                trade_date: parse_naive_date(row.get(1)?)?,
                seat_type: row.get(2)?,
                buy_amount: row.get(3)?,
                sell_amount: row.get(4)?,
                net_amount: row.get(5)?,
                institution_flag: row.get(6)?,
                update_date: parse_naive_date_opt(row.get(7)?)?,
            })
        })
    }

    /// Load 大宗交易 (block trade) rows within `[start, end]` (inclusive).
    ///
    /// Null price/volume/amount are coalesced to `0.0`. If
    /// `block_trade.parquet` doesn't exist, returns an empty vec.
    pub fn fetch_block_trade(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<BlockTradeRow>, DataError> {
        let path = self.parquet_dir.join("block_trade.parquet");
        let sql = format!(
            "SELECT symbol, CAST(trade_date AS VARCHAR) AS trade_date,
                    COALESCE(price, 0.0), COALESCE(volume, 0.0), COALESCE(amount, 0.0),
                    buyer, seller, premium_rate,
                    CAST(update_date AS VARCHAR) AS update_date
             FROM read_parquet('{}')
             WHERE trade_date >= ? AND trade_date <= ?
             ORDER BY symbol, trade_date ASC",
            escape_sql_path(&path.to_string_lossy())
        );
        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();
        self.query_parquet(&path, &sql, params![start_str, end_str], |row| {
            Ok(BlockTradeRow {
                symbol: row.get(0)?,
                trade_date: parse_naive_date(row.get(1)?)?,
                price: row.get(2)?,
                volume: row.get(3)?,
                amount: row.get(4)?,
                buyer: row.get(5)?,
                seller: row.get(6)?,
                premium_rate: row.get(7)?,
                update_date: parse_naive_date_opt(row.get(8)?)?,
            })
        })
    }

    /// Load 机构调研 (institution survey) rows within `[start, end]` (inclusive).
    ///
    /// If `institution_survey.parquet` doesn't exist, returns an empty vec.
    pub fn fetch_institution_survey(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<InstitutionSurveyRow>, DataError> {
        let path = self.parquet_dir.join("institution_survey.parquet");
        let sql = format!(
            "SELECT symbol, CAST(survey_date AS VARCHAR) AS survey_date, org_name, survey_type,
                    CAST(update_date AS VARCHAR) AS update_date
             FROM read_parquet('{}')
             WHERE survey_date >= ? AND survey_date <= ?
             ORDER BY symbol, survey_date ASC",
            escape_sql_path(&path.to_string_lossy())
        );
        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();
        self.query_parquet(&path, &sql, params![start_str, end_str], |row| {
            Ok(InstitutionSurveyRow {
                symbol: row.get(0)?,
                survey_date: parse_naive_date(row.get(1)?)?,
                org_name: row.get(2)?,
                survey_type: row.get(3)?,
                update_date: parse_naive_date_opt(row.get(4)?)?,
            })
        })
    }
}

fn date_str_to_utc(date_str: &str) -> Option<DateTime<Utc>> {
    // Try date-only format (CAST from DATE column)
    if let Ok(naive) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let naive_dt = naive.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    // Try timestamp format (CAST from TIMESTAMP column includes time component)
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    // Handle sub-second precision if present
    if let Ok(naive_dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    None
}

/// Parse a non-null `CAST(... AS VARCHAR)` date cell into a `NaiveDate`.
///
/// Reuses the `date_str_to_utc` DATE/TIMESTAMP tolerance; returns an error
/// instead of silently dropping the row on malformed input.
fn parse_naive_date(date_str: String) -> Result<NaiveDate, duckdb::Error> {
    date_str_to_utc(&date_str)
        .map(|dt| dt.date_naive())
        .ok_or_else(|| {
            duckdb::Error::FromSqlConversionFailure(
                0,
                duckdb::types::Type::Text,
                Box::new(DataError::Parse(format!("invalid date '{date_str}'"))),
            )
        })
}

/// Parse a nullable date cell: `NULL` → `None`, else parse like `parse_naive_date`.
fn parse_naive_date_opt(date_str: Option<String>) -> Result<Option<NaiveDate>, duckdb::Error> {
    date_str.map(parse_naive_date).transpose()
}

// ---------------------------------------------------------------------------
// DataProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl DataProvider for ParquetReader {
    async fn fetch_bars(
        &self,
        symbol: &str,
        _timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        let reader = self.clone_reader();
        let symbol = symbol.to_string();
        tokio::task::spawn_blocking(move || {
            reader.fetch_bars_blocking(&symbol, range_start, range_end)
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        let symbols = self.list_symbols()?;
        if query.is_empty() {
            return Ok(symbols);
        }
        let lower = query.to_lowercase();
        Ok(symbols
            .into_iter()
            .filter(|s| s.code.to_lowercase().contains(&lower))
            .collect())
    }
}

impl Clone for ParquetReader {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            parquet_dir: self.parquet_dir.clone(),
            daily_path: self.daily_path.clone(),
            basic_path: self.basic_path.clone(),
        }
    }
}

impl ParquetReader {
    fn clone_reader(&self) -> Self {
        self.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: create a temporary Parquet dataset with stock_basic rows.
    fn create_test_parquet_dir() -> (tempfile::TempDir, ParquetReader) {
        let tmp = tempfile::tempdir().expect("tempdir");

        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);
             INSERT INTO basic VALUES ('SZ000001', '平安银行', 'SZ', '1991-04-03', NULL, '主板', '平安银行股份有限公司', 19405918198, '银行', '广东省');
             INSERT INTO basic VALUES ('SH600519', '贵州茅台', 'SH', '2001-08-27', NULL, '主板', '贵州茅台酒股份有限公司', 1256197800, '白酒', '贵州省');
             INSERT INTO basic VALUES ('SZ999999', 'hacked', 'SZ', NULL, NULL, NULL, NULL, NULL, NULL, NULL);",
        )
        .expect("create");

        let basic_path = tmp.path().join("stock_basic.parquet");
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            basic_path.display()
        ))
        .expect("copy");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        (tmp, reader)
    }

    /// Helper: create a single `stock_daily.parquet` with a `symbol` column.
    /// `data` is a list of (symbol, [(date_str, close), ...]).
    /// Also writes `stock_daily.symbols.txt` for fast list_symbols().
    fn create_test_stock_daily_parquet(tmp: &tempfile::TempDir, data: &[(&str, &[(&str, f64)])]) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
        ).expect("create");
        for (symbol, rows) in data {
            for (date, close) in *rows {
                conn.execute(
                    "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        *symbol,
                        *date,
                        close - 1.0,
                        close + 1.0,
                        close - 0.5,
                        *close,
                        *close,
                        1000.0,
                        0.0
                    ],
                )
                .expect("insert");
            }
        }
        let path = tmp.path().join("stock_daily.parquet");
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy");

        // Write companion symbols.txt
        let mut symbols: Vec<&str> = data.iter().map(|(s, _)| *s).collect();
        symbols.sort();
        let symbols_txt_path = tmp.path().join("stock_daily.symbols.txt");
        let mut f = std::fs::File::create(&symbols_txt_path).expect("create symbols.txt");
        for s in symbols {
            writeln!(f, "{}", s).expect("write");
        }
    }

    /// `(date_str, close, adjclose)` triplet for the adjclose-aware fixture.
    type AdjcloseTriple<'a> = (&'a str, f64, f64);

    /// Like [`create_test_stock_daily_parquet`] but with an explicit adjclose
    /// per row. `data` is (symbol, [(date_str, close, adjclose), ...]);
    /// open = close - 1, high = close + 1, low = close - 0.5.
    fn create_test_stock_daily_parquet_adjclose(
        tmp: &tempfile::TempDir,
        data: &[(&str, &[AdjcloseTriple])],
    ) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
        ).expect("create");
        for (symbol, rows) in data {
            for (date, close, adjclose) in *rows {
                conn.execute(
                    "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        *symbol,
                        *date,
                        close - 1.0,
                        close + 1.0,
                        close - 0.5,
                        *close,
                        *adjclose,
                        1000.0,
                        0.0
                    ],
                )
                .expect("insert");
            }
        }
        let path = tmp.path().join("stock_daily.parquet");
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy");
    }

    #[test]
    fn parquet_reader_returns_error_for_missing_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        let result = reader.fetch_bars_blocking("SZ000001", start, end);
        assert!(matches!(result, Err(DataError::NoData { .. })));
    }

    /// `fetch_bars_blocking` must carry adjclose out of the parquet query and
    /// scale OHLC by factor = adjclose/close (前复权, ref #176); the latest
    /// bar is the anchor (factor 1.0).
    #[test]
    fn fetch_bars_blocking_scales_ohlc_by_adjclose() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet_adjclose(
            &tmp,
            &[(
                "SZ000001",
                &[("2024-01-02", 10.0, 8.0), ("2024-01-03", 12.0, 12.0)],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        let bars = reader
            .fetch_bars_blocking("SZ000001", start, end)
            .expect("fetch_bars_blocking failed");

        assert_eq!(bars.len(), 2);
        // Older bar: factor = 8/10 = 0.8 → open 9×0.8=7.2, close 10×0.8=8.0.
        assert!((bars[0].open - 7.2).abs() < 1e-9, "scaled open");
        assert!(
            (bars[0].close - 8.0).abs() < 1e-9,
            "scaled close == adjclose"
        );
        // Latest bar: anchor factor 1.0 → unchanged.
        assert_eq!(bars[1].open, 11.0);
        assert_eq!(bars[1].close, 12.0);
    }

    #[test]
    fn list_symbols_returns_empty_when_dir_missing() {
        let reader = ParquetReader::new("/nonexistent/path").expect("create reader");
        let symbols = reader.list_symbols().expect("list symbols");
        assert!(symbols.is_empty());
    }

    #[test]
    fn list_symbols_reads_from_symbols_txt() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-01", 10.0)]),
                ("SH600519", &[("2024-01-01", 1500.0)]),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.list_symbols().expect("list");
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].code, "SH600519");
        assert_eq!(symbols[1].code, "SZ000001");
    }

    #[test]
    fn fetch_cross_section_returns_all_market_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-02", 10.0), ("2024-01-03", 10.5)]),
                (
                    "SH600519",
                    &[("2024-01-02", 1500.0), ("2024-01-03", 1520.0)],
                ),
                ("BJ920992", &[("2024-01-02", 8.0)]),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let bars = reader
            .fetch_cross_section(
                NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
            )
            .expect("fetch cross section");

        assert_eq!(bars.len(), 5, "all market rows within range");
        // Ordered by symbol (BJ < SH < SZ), then trade_date.
        assert_eq!(bars[0].symbol, "BJ920992");
        assert_eq!(
            bars[0].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("date")
        );
        assert_eq!(bars[0].adjclose, 8.0);
        assert_eq!(bars[0].close, 8.0);
        assert_eq!(bars[0].volume, 1000.0);
        // Fixture derives OHLC from close: open=close-1, high=close+1, low=close-0.5.
        assert_eq!(bars[0].open, 7.0);
        assert_eq!(bars[0].high, 9.0);
        assert_eq!(bars[0].low, 7.5);
        assert_eq!(bars[0].amount, 0.0);
        assert_eq!(bars[1].symbol, "SH600519");
        assert_eq!(
            bars[1].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("date")
        );
        assert_eq!(bars[1].adjclose, 1500.0);
        assert_eq!(
            bars[3].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("date")
        );
        assert_eq!(bars[3].symbol, "SZ000001");
        assert_eq!(bars[3].adjclose, 10.0);
        assert_eq!(
            bars[4].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 3).expect("date")
        );
        assert_eq!(bars[4].symbol, "SZ000001");
        assert_eq!(bars[4].adjclose, 10.5);
    }

    #[test]
    fn fetch_cross_section_returns_all_nine_fields() {
        // Explicit OHLCV/amount values (not fixture-derived) to pin every column.
        let tmp = tempfile::tempdir().expect("tempdir");
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);
             INSERT INTO t VALUES ('000001', '2024-01-02', 9.5, 11.2, 8.8, 10.3, 10.3, 1234567.0, 987654321.0);",
        )
        .expect("create");
        let path = tmp.path().join("stock_daily.parquet");
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let bars = reader
            .fetch_cross_section(
                NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
            )
            .expect("fetch cross section");
        assert_eq!(bars.len(), 1);
        let b = &bars[0];
        assert_eq!(b.open, 9.5);
        assert_eq!(b.high, 11.2);
        assert_eq!(b.low, 8.8);
        assert_eq!(b.close, 10.3);
        assert_eq!(b.adjclose, 10.3);
        assert_eq!(b.volume, 1_234_567.0);
        assert_eq!(b.amount, 987_654_321.0);
    }

    // -----------------------------------------------------------------------
    // SEPA table read primitives (epic #139 decision 13)
    // -----------------------------------------------------------------------

    /// Create `<name>.parquet` in `tmp` from raw DDL + INSERT statements,
    /// mirroring the column layout the import pipeline writes.
    fn create_test_table_parquet(tmp: &tempfile::TempDir, name: &str, ddl: &str, inserts: &[&str]) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(ddl).expect("create");
        for ins in inserts {
            conn.execute_batch(ins).expect("insert");
        }
        conn.execute_batch(&format!(
            "COPY {name} TO '{}' (FORMAT PARQUET)",
            tmp.path().join(format!("{name}.parquet")).display()
        ))
        .expect("copy");
    }

    #[test]
    fn fetch_concept_member_returns_all_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "concept_member",
            "CREATE TABLE concept_member (concept_code VARCHAR, symbol VARCHAR, concept_name VARCHAR, update_date DATE)",
            &[
                "INSERT INTO concept_member VALUES ('BK1169', 'SH600519', 'Kimi概念', '2026-08-01')",
                "INSERT INTO concept_member VALUES ('BK1169', 'SZ000001', 'Kimi概念', '2026-08-01')",
                "INSERT INTO concept_member VALUES ('BK0800', 'SH601127', NULL, '2026-08-01')",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader.fetch_concept_member().expect("fetch concept member");
        assert_eq!(rows.len(), 3);
        // Ordered by concept_code, then symbol: BK0800 < BK1169.
        assert_eq!(rows[0].concept_code, "BK0800");
        assert_eq!(rows[0].symbol, "SH601127");
        assert_eq!(rows[0].concept_name, None, "NULL concept_name is None");
        let kimi = rows
            .iter()
            .find(|r| r.concept_code == "BK1169")
            .expect("row");
        assert_eq!(kimi.symbol, "SH600519");
        assert_eq!(kimi.concept_name.as_deref(), Some("Kimi概念"));
        assert_eq!(
            kimi.update_date,
            Some(NaiveDate::from_ymd_opt(2026, 8, 1).expect("date"))
        );
    }

    #[test]
    fn fetch_capital_main_flow_filters_by_date_range() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "capital_main_flow",
            "CREATE TABLE capital_main_flow (symbol VARCHAR, trade_date DATE, main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, super_large_net DOUBLE, large_net DOUBLE, medium_net DOUBLE, small_net DOUBLE, update_date DATE)",
            &[
                "INSERT INTO capital_main_flow VALUES ('SH600519', '2026-07-28', 1.2e8, 3.5, 8.0e7, 4.0e7, -5.0e6, -1.0e7, '2026-07-28')",
                "INSERT INTO capital_main_flow VALUES ('SH600519', '2026-07-27', 5.0e7, 1.5, 3.0e7, 2.0e7, 0.0, NULL, '2026-07-27')",
                "INSERT INTO capital_main_flow VALUES ('SZ000001', '2026-07-28', -2.0e7, -1.0, -1.0e7, -1.0e7, 0.0, 0.0, '2026-07-28')",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader
            .fetch_capital_main_flow(
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
            )
            .expect("fetch main flow");
        assert_eq!(rows.len(), 2, "only in-range rows");
        let r = rows.iter().find(|r| r.symbol == "SH600519").expect("row");
        assert_eq!(r.main_net_inflow, 1.2e8);
        assert_eq!(r.main_net_inflow_rate, 3.5);
        assert_eq!(r.super_large_net, 8.0e7);
        assert_eq!(r.large_net, 4.0e7);
        assert_eq!(r.medium_net, -5.0e6);
        assert_eq!(r.small_net, Some(-1.0e7));
        assert_eq!(
            r.update_date,
            Some(NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"))
        );
    }

    #[test]
    fn fetch_capital_main_flow_coalesces_null_into_zero() {
        // DDL allows NULLs; the reader must map them to 0.0, not fail.
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "capital_main_flow",
            "CREATE TABLE capital_main_flow (symbol VARCHAR, trade_date DATE, main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, super_large_net DOUBLE, large_net DOUBLE, medium_net DOUBLE, small_net DOUBLE, update_date DATE)",
            &[
                "INSERT INTO capital_main_flow VALUES ('SZ000001', '2026-07-28', NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader
            .fetch_capital_main_flow(
                NaiveDate::from_ymd_opt(2026, 7, 1).expect("date"),
                NaiveDate::from_ymd_opt(2026, 7, 31).expect("date"),
            )
            .expect("fetch main flow");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].main_net_inflow, 0.0, "NULL coalesced to 0.0");
        assert_eq!(rows[0].small_net, None, "small_net stays Option");
        assert_eq!(rows[0].update_date, None, "NULL update_date is None");
    }

    #[test]
    fn fetch_dragon_list_returns_rows_with_institution_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "dragon_list",
            "CREATE TABLE dragon_list (symbol VARCHAR, trade_date DATE, seat_type VARCHAR, buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE, institution_flag TINYINT, update_date DATE)",
            &[
                "INSERT INTO dragon_list VALUES ('SH600519', '2026-07-28', '机构专用', 5.0e8, 1.0e8, 4.0e8, 1, '2026-07-28')",
                "INSERT INTO dragon_list VALUES ('SH600519', '2026-07-28', '东方财富拉萨', 2.0e8, 3.0e8, NULL, 0, '2026-07-28')",
                "INSERT INTO dragon_list VALUES ('SZ000001', '2026-07-27', '机构专用', NULL, NULL, NULL, NULL, '2026-07-27')",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader
            .fetch_dragon_list(
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
            )
            .expect("fetch dragon list");
        assert_eq!(rows.len(), 2, "only in-range rows");
        let inst = rows
            .iter()
            .find(|r| r.seat_type == "机构专用")
            .expect("row");
        assert_eq!(inst.buy_amount, 5.0e8);
        assert_eq!(inst.sell_amount, 1.0e8);
        assert_eq!(inst.net_amount, Some(4.0e8));
        assert_eq!(inst.institution_flag, Some(1));
        let seat = rows
            .iter()
            .find(|r| r.seat_type == "东方财富拉萨")
            .expect("row");
        assert_eq!(seat.net_amount, None);
        assert_eq!(seat.institution_flag, Some(0));
    }

    #[test]
    fn fetch_block_trade_returns_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "block_trade",
            "CREATE TABLE block_trade (symbol VARCHAR, trade_date DATE, price DOUBLE, volume DOUBLE, amount DOUBLE, buyer VARCHAR, seller VARCHAR, premium_rate DOUBLE, update_date DATE)",
            &[
                "INSERT INTO block_trade VALUES ('SH600519', '2026-07-28', 1495.0, 100000.0, 1.495e8, '中信证券总部', '机构专用', -1.8, '2026-07-28')",
                "INSERT INTO block_trade VALUES ('SZ000001', '2026-07-28', 9.8, 200000.0, 1.96e6, NULL, NULL, NULL, '2026-07-28')",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader
            .fetch_block_trade(
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
            )
            .expect("fetch block trade");
        assert_eq!(rows.len(), 2);
        let r = &rows[0];
        assert_eq!(r.price, 1495.0);
        assert_eq!(r.volume, 100_000.0);
        assert_eq!(r.amount, 1.495e8);
        assert_eq!(r.buyer.as_deref(), Some("中信证券总部"));
        assert_eq!(r.seller.as_deref(), Some("机构专用"));
        assert_eq!(r.premium_rate, Some(-1.8));
        assert_eq!(rows[1].buyer, None, "NULL buyer is None");
        assert_eq!(rows[1].premium_rate, None);
    }

    #[test]
    fn fetch_institution_survey_returns_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_table_parquet(
            &tmp,
            "institution_survey",
            "CREATE TABLE institution_survey (symbol VARCHAR, survey_date DATE, org_name VARCHAR, survey_type VARCHAR, update_date DATE)",
            &[
                "INSERT INTO institution_survey VALUES ('SH600519', '2026-07-28', '长信基金', '电话会议', '2026-07-28')",
                "INSERT INTO institution_survey VALUES ('SH600519', '2026-07-28', '国泰基金', NULL, '2026-07-28')",
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let rows = reader
            .fetch_institution_survey(
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
                NaiveDate::from_ymd_opt(2026, 7, 28).expect("date"),
            )
            .expect("fetch institution survey");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].org_name, "长信基金");
        assert_eq!(rows[0].survey_type.as_deref(), Some("电话会议"));
        assert_eq!(rows[1].survey_type, None, "NULL survey_type is None");
    }

    #[test]
    fn sepa_table_readers_return_empty_when_parquet_missing() {
        // Boundary: GUI tables not yet imported must degrade to empty vecs,
        // not DataError — otherwise run_sepa's `?` fails hard (review revision).
        let tmp = tempfile::tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        assert!(reader.fetch_concept_member().expect("empty").is_empty());
        assert!(
            reader
                .fetch_capital_main_flow(
                    NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                    NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
                )
                .expect("empty")
                .is_empty()
        );
        assert!(
            reader
                .fetch_dragon_list(
                    NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                    NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
                )
                .expect("empty")
                .is_empty()
        );
        assert!(
            reader
                .fetch_block_trade(
                    NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                    NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
                )
                .expect("empty")
                .is_empty()
        );
        assert!(
            reader
                .fetch_institution_survey(
                    NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                    NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
                )
                .expect("empty")
                .is_empty()
        );
    }

    #[test]
    fn fetch_cross_section_filters_by_date_range() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[(
                "000001",
                &[
                    ("2024-01-02", 10.0),
                    ("2024-01-03", 10.5),
                    ("2024-01-04", 11.0),
                ],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let bars = reader
            .fetch_cross_section(
                NaiveDate::from_ymd_opt(2024, 1, 3).expect("date"),
                NaiveDate::from_ymd_opt(2024, 1, 3).expect("date"),
            )
            .expect("fetch cross section");

        assert_eq!(bars.len(), 1, "only in-range rows, boundaries inclusive");
        assert_eq!(
            bars[0].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 3).expect("date")
        );
    }

    #[test]
    fn fetch_cross_section_returns_empty_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let bars = reader
            .fetch_cross_section(
                NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
            )
            .expect("empty vec, not error");
        assert!(bars.is_empty());
    }

    #[test]
    fn fetch_cross_section_parses_timestamp_tradedate() {
        // Regression guard: real stock_daily.parquet stores tradedate as
        // TIMESTAMP; CAST AS VARCHAR yields "2024-01-02 00:00:00" with a time
        // component. Parsing must handle it (date_str_to_utc does).
        let tmp = tempfile::tempdir().expect("tempdir");
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);
             INSERT INTO t VALUES ('000001', TIMESTAMP '2024-01-02 00:00:00', 9.0, 11.0, 9.5, 10.0, 10.0, 1000.0, 0.0);",
        )
        .expect("create");
        let path = tmp.path().join("stock_daily.parquet");
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let bars = reader
            .fetch_cross_section(
                NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
                NaiveDate::from_ymd_opt(2030, 1, 1).expect("date"),
            )
            .expect("fetch cross section");
        assert_eq!(bars.len(), 1);
        assert_eq!(
            bars[0].trade_date,
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("date")
        );
        assert_eq!(bars[0].adjclose, 10.0);
    }

    #[test]
    fn sql_injection_via_symbol_is_blocked() {
        let (_tmp, reader) = create_test_parquet_dir();

        // Malicious symbol that would cause SQL issues if not parameterized.
        // Since symbols are bound as DuckDB parameters, injection is not possible.
        let malicious = "' OR 1=1 --";
        let result = reader.get_stock_basic_blocking(malicious);
        assert!(
            result.is_err() || result.unwrap().is_none(),
            "SQL injection via symbol should return error or None, not data"
        );
    }

    #[test]
    fn validate_symbol_allows_canonical_prefixed_codes() {
        // Canonical Dolt-native form: uppercase exchange prefix + 6 digits
        validate_symbol("SZ000001").expect("SZ000001 should be valid");
        validate_symbol("SH600519").expect("SH600519 should be valid");
        validate_symbol("BJ830799").expect("BJ830799 should be valid");
        // Bare codes, dot forms and any alnum+dot shapes are rejected (D9:
        // the data access layer only accepts the canonical prefixed form)
        assert!(validate_symbol("000001").is_err(), "bare code rejected");
        assert!(validate_symbol("600519").is_err(), "bare code rejected");
        assert!(validate_symbol("sh.000001").is_err(), "dot form rejected");
        assert!(validate_symbol("sz.600059").is_err(), "dot form rejected");
        assert!(
            validate_symbol("sz000001").is_err(),
            "lowercase prefix rejected"
        );
        // Empty and special chars still rejected
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("DROP TABLE").is_err());
        assert!(validate_symbol("foo;bar").is_err());
    }

    #[test]
    fn validate_symbol_rejects_slashes() {
        // Slashes are not alphanumeric, so they are rejected
        assert!(validate_symbol("../../etc/passwd").is_err());
    }

    #[test]
    fn get_stock_basic_returns_correct_row() {
        let (_tmp, reader) = create_test_parquet_dir();

        let info = reader
            .get_stock_basic_blocking("SZ000001")
            .expect("should succeed")
            .expect("should find SZ000001");
        assert_eq!(info.symbol, "SZ000001");
        assert_eq!(info.name, "平安银行");
        assert_eq!(info.area.as_deref(), Some("广东省"));
        assert_eq!(info.industry.as_deref(), Some("银行"));
        assert_eq!(info.board.as_deref(), Some("主板"));
        assert_eq!(info.full_name.as_deref(), Some("平安银行股份有限公司"));
        assert_eq!(info.total_share, Some(19_405_918_198.0));
        assert_eq!(
            info.list_date.map(|d| d.to_string()),
            Some("1991-04-03".to_string())
        );
        assert_eq!(info.delist_date, None);

        let info2 = reader
            .get_stock_basic_blocking("SH600519")
            .expect("should succeed")
            .expect("should find SH600519");
        assert_eq!(info2.symbol, "SH600519");
        assert_eq!(info2.area.as_deref(), Some("贵州省"));
        assert_eq!(info2.board.as_deref(), Some("主板"));
        assert_eq!(info2.total_share, Some(1_256_197_800.0));

        let info3 = reader
            .get_stock_basic_blocking("SZ999999")
            .expect("should succeed")
            .expect("should find SZ999999");
        assert_eq!(info3.symbol, "SZ999999");
        assert_eq!(info3.area, None);
        assert_eq!(info3.industry, None);
        assert_eq!(info3.board, None);
        assert_eq!(info3.full_name, None);
        assert_eq!(info3.total_share, None);
    }

    // -----------------------------------------------------------------------
    // Happy-path tests with real temp Parquet OHLCV data
    // -----------------------------------------------------------------------

    #[test]
    fn fetch_bars_returns_sorted_ohlcv() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[(
                "SZ000001",
                &[
                    ("2024-01-02", 10.0),
                    ("2024-01-03", 11.0),
                    ("2024-01-04", 10.5),
                ],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();
        let bars = reader
            .fetch_bars_blocking("SZ000001", start, end)
            .expect("fetch");

        assert_eq!(bars.len(), 3);
        assert!((bars[0].open - 9.0).abs() < 0.01);
        assert!((bars[0].close - 10.0).abs() < 0.01);
        assert!((bars[1].close - 11.0).abs() < 0.01);
        assert!(bars[0].time <= bars[1].time, "should be sorted ascending");
    }

    #[test]
    fn fetch_bars_filters_by_date_range() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[(
                "SZ000001",
                &[
                    ("2024-01-02", 10.0),
                    ("2024-01-03", 11.0),
                    ("2024-02-01", 12.0),
                ],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_naive_utc_and_offset(
            chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            Utc,
        );
        let end = DateTime::from_naive_utc_and_offset(
            chrono::NaiveDate::from_ymd_opt(2024, 1, 31)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            Utc,
        );
        let bars = reader
            .fetch_bars_blocking("SZ000001", start, end)
            .expect("fetch");
        assert_eq!(bars.len(), 2, "should only return Jan dates");
    }

    #[test]
    fn fetch_bars_only_returns_requested_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-02", 10.0)]),
                ("SH600519", &[("2024-01-02", 1500.0)]),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        // Should only get SZ000001 rows, not SH600519
        let bars = reader
            .fetch_bars_blocking("SZ000001", start, end)
            .expect("fetch");
        assert_eq!(bars.len(), 1);
        assert!((bars[0].close - 10.0).abs() < 0.01);
    }

    #[test]
    fn get_stored_range_returns_min_max() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[(
                "SH600519",
                &[
                    ("2024-06-01", 1500.0),
                    ("2024-06-15", 1510.0),
                    ("2024-06-30", 1520.0),
                ],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range = reader
            .get_stored_range("SH600519")
            .expect("range")
            .expect("some");
        assert_eq!(range.0.to_string(), "2024-06-01");
        assert_eq!(range.1.to_string(), "2024-06-30");
    }

    #[test]
    fn latest_trade_date_returns_max() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[(
                "SH600519",
                &[
                    ("2024-06-01", 1500.0),
                    ("2024-06-15", 1510.0),
                    ("2024-06-30", 1520.0),
                ],
            )],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let date = reader.latest_trade_date().expect("date").expect("some");
        assert_eq!(date.to_string(), "2024-06-30");
    }

    #[test]
    fn latest_trade_date_none_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        assert!(reader.latest_trade_date().expect("no error").is_none());
    }

    #[test]
    fn list_symbols_falls_back_to_sql_when_symbols_txt_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Create stock_daily.parquet WITHOUT symbols.txt
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);
             INSERT INTO t VALUES ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0);
             INSERT INTO t VALUES ('SH600519', '2024-01-02', 1499, 1501, 1498, 1500, 1500, 2000, 0);",
        ).expect("create");
        conn.execute_batch(&format!(
            "COPY t TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.list_symbols().expect("list");
        assert_eq!(
            symbols.len(),
            2,
            "should find both symbols via SQL fallback"
        );
        assert!(symbols.iter().any(|s| s.code == "SZ000001"));
        assert!(symbols.iter().any(|s| s.code == "SH600519"));
    }

    #[test]
    fn get_stored_range_filters_by_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-02", 10.0), ("2024-01-03", 11.0)]),
                (
                    "SH600519",
                    &[("2024-06-01", 1500.0), ("2024-06-30", 1520.0)],
                ),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range_01 = reader
            .get_stored_range("SZ000001")
            .expect("range")
            .expect("some");
        assert_eq!(range_01.0.to_string(), "2024-01-02");
        assert_eq!(range_01.1.to_string(), "2024-01-03");

        let range_519 = reader
            .get_stored_range("SH600519")
            .expect("range")
            .expect("some");
        assert_eq!(range_519.0.to_string(), "2024-06-01");
        assert_eq!(range_519.1.to_string(), "2024-06-30");
    }

    #[test]
    fn get_stored_range_returns_none_for_missing_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(&tmp, &[("SZ000001", &[("2024-01-02", 10.0)])]);

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range = reader.get_stored_range("SZ999999").expect("range");
        assert!(range.is_none(), "nonexistent symbol should return None");
    }

    // -----------------------------------------------------------------------
    // load_all_stock_basics
    // -----------------------------------------------------------------------

    #[test]
    fn load_all_stock_basics_returns_all_rows() {
        let (_tmp, reader) = create_test_parquet_dir();

        let basics = reader.load_all_stock_basics().expect("load");
        assert_eq!(basics.len(), 3, "should return all 3 rows");

        // Ordered by symbol: SZ000001, SH600519, SZ999999
        let pab = basics
            .iter()
            .find(|b| b.symbol == "SZ000001")
            .expect("find SZ000001");
        assert_eq!(pab.name, "平安银行");
        assert!(
            pab.list_date.is_some(),
            "list_date should be Some for SZ000001"
        );

        let hack = basics
            .iter()
            .find(|b| b.symbol == "SZ999999")
            .expect("find SZ999999");
        assert!(hack.list_date.is_none(), "NULL list_date should be None");
        assert!(
            hack.delist_date.is_none(),
            "NULL delist_date should be None"
        );
    }

    #[test]
    fn load_all_stock_basics_returns_empty_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let basics = reader.load_all_stock_basics().expect("load");
        assert!(
            basics.is_empty(),
            "should return empty vec when stock_basic.parquet missing"
        );
    }

    // -----------------------------------------------------------------------
    // get_stock_basic_blocking edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn get_stock_basic_blocking_returns_nodata_for_valid_but_unknown_symbol() {
        let (_tmp, reader) = create_test_parquet_dir();
        let result = reader.get_stock_basic_blocking("999999");
        assert!(
            matches!(result, Err(DataError::NoData { .. })),
            "valid symbol with no matching row should return NoData"
        );
    }

    #[test]
    fn get_stock_basic_blocking_returns_none_when_basic_path_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let result = reader
            .get_stock_basic_blocking("SZ000001")
            .expect("should be Ok");
        assert!(
            result.is_none(),
            "missing basic_path should return Ok(None)"
        );
    }

    // -----------------------------------------------------------------------
    // fetch_bars_blocking edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fetch_bars_blocking_returns_nodata_for_zero_results() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(&tmp, &[("SZ000001", &[("2024-01-02", 10.0)])]);
        let reader = ParquetReader::new(tmp.path()).expect("create reader");

        // Date range outside any data
        let start = DateTime::from_naive_utc_and_offset(
            NaiveDate::from_ymd_opt(2025, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            Utc,
        );
        let end = DateTime::from_naive_utc_and_offset(
            NaiveDate::from_ymd_opt(2025, 12, 31)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            Utc,
        );
        let result = reader.fetch_bars_blocking("SZ000001", start, end);
        assert!(
            matches!(result, Err(DataError::NoData { .. })),
            "zero results in date range should return NoData"
        );
    }

    #[test]
    fn fetch_bars_blocking_rejects_invalid_symbol_even_when_data_exists() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(&tmp, &[("SZ000001", &[("2024-01-02", 10.0)])]);
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        // Empty string is invalid and should fail validate_symbol BEFORE any I/O
        let result = reader.fetch_bars_blocking("", start, end);
        assert!(
            matches!(result, Err(DataError::NoData { .. })),
            "invalid symbol (empty) should be rejected by validate_symbol"
        );
    }

    // -----------------------------------------------------------------------
    // escape_sql_path
    // -----------------------------------------------------------------------

    #[test]
    fn escape_sql_path_doubles_single_quotes() {
        let input = "/path/with/'quote'/file.parquet";
        let escaped = escape_sql_path(input);
        assert_eq!(escaped, "/path/with/''quote''/file.parquet");
    }

    #[test]
    fn escape_sql_path_passes_through_no_quotes() {
        let input = "/normal/path/file.parquet";
        let escaped = escape_sql_path(input);
        assert_eq!(escaped, input);
    }

    // -----------------------------------------------------------------------
    // search_symbols (async trait impl)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn search_symbols_empty_query_returns_all() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-02", 10.0)]),
                ("SH600519", &[("2024-01-02", 1500.0)]),
            ],
        );
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.search_symbols("").await.expect("search");
        assert_eq!(symbols.len(), 2);
        // list_symbols sorts alphabetically: SH600519, SZ000001
        assert_eq!(symbols[0].code, "SH600519");
        assert_eq!(symbols[1].code, "SZ000001");
    }

    #[tokio::test]
    async fn search_symbols_filters_by_partial_match() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[
                ("SZ000001", &[("2024-01-02", 10.0)]),
                ("SH600519", &[("2024-01-02", 1500.0)]),
            ],
        );
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.search_symbols("600519").await.expect("search");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].code, "SH600519");
    }

    #[tokio::test]
    async fn search_symbols_case_insensitive_filter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(&tmp, &[("SZ000001", &[("2024-01-02", 10.0)])]);
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.search_symbols("sz").await.expect("search");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].code, "SZ000001");
    }

    // -----------------------------------------------------------------------
    // fetch_bars (async trait impl via spawn_blocking)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_bars_async_delegates_to_blocking() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(&tmp, &[("SZ000001", &[("2024-01-02", 10.0)])]);
        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();
        let bars = reader
            .fetch_bars("SZ000001", "1d", start, end)
            .await
            .expect("fetch");
        assert_eq!(bars.len(), 1);
        assert!((bars[0].close - 10.0).abs() < 0.01);
    }
}
