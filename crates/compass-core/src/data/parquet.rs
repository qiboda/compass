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
use crate::model::{StockBasic, SymbolInfo};

/// Validate symbol for use in DuckDB parameter bindings.
///
/// With the single-file format, symbols are bound as DuckDB parameters (`?`),
/// not inserted into SQL strings. This function provides defense-in-depth:
/// allows alphanumeric chars plus `.` (for exchange-prefixed symbols like
/// `sh.600058`). Rejects empty strings and other special chars.
pub(crate) fn validate_symbol(symbol: &str) -> Result<&str, DataError> {
    let valid = !symbol.is_empty()
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.');
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
            daily_path: dir.join("stock_daily.parquet"),
            basic_path: dir.join("stock_basic.parquet"),
        })
    }

    /// Fetch bars for a symbol and date range from the single Parquet file.
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
            "SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume
             FROM read_parquet('{escaped}')
             WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
             ORDER BY tradedate ASC"
        );
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<(String, f64, f64, f64, f64, f64)> = stmt
            .query_map(params![symbol, start_str, end_str], |row| {
                Ok((
                    row.get(0)?,
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
                let time = date_str_to_utc(&date_str)?;
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

        let sql = format!(
            "SELECT DISTINCT symbol FROM read_parquet('{escaped}') ORDER BY symbol"
        );
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
            "SELECT symbol, name, exchange, CAST(list_date AS VARCHAR), CAST(delist_date AS VARCHAR)
             FROM read_parquet('{escaped}')
             WHERE symbol = ?"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let result = stmt
            .query_row(params![symbol], |row| {
                Ok(StockBasic {
                    symbol: row.get(0)?,
                    name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    area: None,
                    industry: None,
                    market: None,
                    exchange: row.get::<_, Option<String>>(2)?,
                    list_date: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                    delist_date: row
                        .get::<_, Option<String>>(4)?
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
    /// Returns the full stock list (symbol, name, exchange) for use in
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
            "SELECT symbol, name, exchange, CAST(list_date AS VARCHAR), CAST(delist_date AS VARCHAR)
             FROM read_parquet('{escaped}')
             ORDER BY symbol"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<StockBasic> = stmt
            .query_map([], |row| {
                Ok(StockBasic {
                    symbol: row.get(0)?,
                    name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    exchange: row.get::<_, Option<String>>(2)?,
                    area: None,
                    industry: None,
                    market: None,
                    list_date: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                    delist_date: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| date_str_to_utc(&s).map(|dt| dt.date_naive())),
                })
            })
            .map_err(DataError::Database)?
            .collect::<Result<Vec<_>, duckdb::Error>>()
            .map_err(DataError::Database)?;

        Ok(rows)
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
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE);
             INSERT INTO basic VALUES ('000001', '平安银行', 'SZ', '1991-04-03', NULL);
             INSERT INTO basic VALUES ('600519', '贵州茅台', 'SH', '2001-08-27', NULL);
             INSERT INTO basic VALUES ('hack', 'hacked', 'XX', NULL, NULL);",
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
    fn create_test_stock_daily_parquet(
        tmp: &tempfile::TempDir,
        data: &[(&str, &[(&str, f64)])],
    ) {
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

    #[test]
    fn parquet_reader_returns_error_for_missing_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        let result = reader.fetch_bars_blocking("SZ000001", start, end);
        assert!(matches!(result, Err(DataError::NoData { .. })));
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
    fn validate_symbol_allows_valid_codes() {
        // Dolt without dot: uppercase prefix + bare code
        validate_symbol("SZ000001").expect("SZ000001 should be valid");
        validate_symbol("SH600519").expect("SH600519 should be valid");
        validate_symbol("BJ830799").expect("BJ830799 should be valid");
        // Exchange-prefixed with dot: lowercase prefix.bare code
        validate_symbol("sh.000001").expect("sh.000001 should be valid");
        validate_symbol("sz.600059").expect("sz.600059 should be valid");
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
            .get_stock_basic_blocking("000001")
            .expect("should succeed")
            .expect("should find 000001");
        assert_eq!(info.symbol, "000001");
        assert_eq!(info.name, "平安银行");
        assert_eq!(info.exchange.as_deref(), Some("SZ"));

        let info2 = reader
            .get_stock_basic_blocking("600519")
            .expect("should succeed")
            .expect("should find 600519");
        assert_eq!(info2.symbol, "600519");
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
        )).expect("copy");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.list_symbols().expect("list");
        assert_eq!(symbols.len(), 2, "should find both symbols via SQL fallback");
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
                ("SH600519", &[("2024-06-01", 1500.0), ("2024-06-30", 1520.0)]),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range_01 = reader.get_stored_range("SZ000001").expect("range").expect("some");
        assert_eq!(range_01.0.to_string(), "2024-01-02");
        assert_eq!(range_01.1.to_string(), "2024-01-03");

        let range_519 = reader.get_stored_range("SH600519").expect("range").expect("some");
        assert_eq!(range_519.0.to_string(), "2024-06-01");
        assert_eq!(range_519.1.to_string(), "2024-06-30");
    }

    #[test]
    fn get_stored_range_returns_none_for_missing_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");
        create_test_stock_daily_parquet(
            &tmp,
            &[("SZ000001", &[("2024-01-02", 10.0)])],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range = reader.get_stored_range("NONEXIST").expect("range");
        assert!(range.is_none(), "nonexistent symbol should return None");
    }
}
