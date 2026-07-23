use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use duckdb::{Connection, OptionalExt, params};
use egui_charts::model::Bar;

use crate::data::provider::{DataError, DataProvider};
use crate::model::{StockBasic, SymbolInfo};

/// Reject symbols that contain non-alphanumeric characters to prevent
/// SQL injection and path traversal via `read_parquet()` file paths.
fn validate_symbol(symbol: &str) -> Result<&str, DataError> {
    if symbol.is_empty() || !symbol.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(DataError::NoData {
            symbol: symbol.to_string(),
        });
    }
    Ok(symbol)
}

/// Read A-share OHLCV data from Parquet files partitioned by symbol.
///
/// Expected directory layout:
/// ```text
/// parquet_data/
///   stock_basic.parquet
///   stock_daily/
///     000001.parquet
///     600519.parquet
///     ...
/// ```
pub struct ParquetReader {
    conn: Arc<Mutex<Connection>>,
    daily_dir: PathBuf,
    basic_path: PathBuf,
}

impl ParquetReader {
    /// Create a new reader pointing at `parquet_dir`.
    ///
    /// The directory must contain `stock_basic.parquet` and a `stock_daily/`
    /// subdirectory with per-symbol Parquet files.
    pub fn new(parquet_dir: impl AsRef<Path>) -> Result<Self, DataError> {
        let dir = parquet_dir.as_ref();
        let conn = Connection::open_in_memory().map_err(DataError::Database)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            daily_dir: dir.join("stock_daily"),
            basic_path: dir.join("stock_basic.parquet"),
        })
    }

    /// Return the parquet file path for a given symbol.
    fn parquet_path(&self, symbol: &str) -> PathBuf {
        self.daily_dir.join(format!("{symbol}.parquet"))
    }

    /// Check whether a parquet file exists for this symbol.
    fn file_exists(&self, symbol: &str) -> bool {
        self.parquet_path(symbol).exists()
    }

    /// Fetch bars for a symbol and date range from the per-symbol Parquet file.
    pub fn fetch_bars_blocking(
        &self,
        symbol: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        validate_symbol(symbol)?;
        let path = self.parquet_path(symbol);
        if !path.exists() {
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        let path_str = path.to_string_lossy();
        let start_str = range_start.format("%Y-%m-%d").to_string();
        let end_str = range_end.format("%Y-%m-%d").to_string();

        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume
             FROM read_parquet('{path_str}')
             WHERE tradedate >= ? AND tradedate <= ?
             ORDER BY tradedate ASC"
        );
        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let rows: Vec<(String, f64, f64, f64, f64, f64)> = stmt
            .query_map(params![start_str, end_str], |row| {
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

    /// List all available symbols (from filesystem, fast).
    pub fn list_symbols(&self) -> Result<Vec<SymbolInfo>, DataError> {
        let mut symbols = Vec::new();

        let dir = match std::fs::read_dir(&self.daily_dir) {
            Ok(d) => d,
            Err(_) => return Ok(symbols),
        };

        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "parquet")
                && let Some(stem) = path.file_stem()
            {
                let code = stem.to_string_lossy().to_string();
                if !code.is_empty() {
                    symbols.push(SymbolInfo {
                        code,
                        name: String::new(),
                    });
                }
            }
        }

        symbols.sort_by(|a, b| a.code.cmp(&b.code));
        Ok(symbols)
    }

    /// Get stored date range for a symbol from its Parquet file.
    pub fn get_stored_range(
        &self,
        symbol: &str,
    ) -> Result<Option<(NaiveDate, NaiveDate)>, DataError> {
        validate_symbol(symbol)?;
        if !self.file_exists(symbol) {
            return Ok(None);
        }

        let path_str = self.parquet_path(symbol).to_string_lossy().to_string();
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT CAST(MIN(tradedate) AS VARCHAR), CAST(MAX(tradedate) AS VARCHAR)
             FROM read_parquet('{path_str}')"
        );

        let mut stmt = conn.prepare(&sql).map_err(DataError::Database)?;
        let result = stmt
            .query_row([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .optional()
            .map_err(DataError::Database)?;

        match result {
            Some((Some(min_s), Some(max_s))) if !min_s.is_empty() && !max_s.is_empty() => {
                let min_date = NaiveDate::parse_from_str(&min_s, "%Y-%m-%d")
                    .map_err(|e| DataError::Parse(format!("invalid date '{min_s}': {e}")))?;
                let max_date = NaiveDate::parse_from_str(&max_s, "%Y-%m-%d")
                    .map_err(|e| DataError::Parse(format!("invalid date '{max_s}': {e}")))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;

        let sql = format!(
            "SELECT symbol, name, exchange, CAST(list_date AS VARCHAR), CAST(delist_date AS VARCHAR)
             FROM read_parquet('{path_str}')
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
                        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                    delist_date: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
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
}

fn date_str_to_utc(date_str: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let naive_dt = naive.and_hms_opt(0, 0, 0)?;
    Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc))
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
            daily_dir: self.daily_dir.clone(),
            basic_path: self.basic_path.clone(),
        }
    }
}

impl ParquetReader {
    #[allow(dead_code)]
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

    /// Helper: create a temporary Parquet dataset with one stock_basic row.
    fn create_test_parquet_dir() -> (tempfile::TempDir, ParquetReader) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let daily_dir = tmp.path().join("stock_daily");
        std::fs::create_dir_all(&daily_dir).expect("mkdir");

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

    #[test]
    fn parquet_reader_returns_error_for_missing_symbol() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();

        let result = reader.fetch_bars_blocking("000001", start, end);
        assert!(matches!(result, Err(DataError::NoData { .. })));
    }

    #[test]
    fn list_symbols_returns_empty_when_dir_missing() {
        let reader = ParquetReader::new("/nonexistent/path").expect("create reader");
        let symbols = reader.list_symbols().expect("list symbols");
        assert!(symbols.is_empty());
    }

    #[test]
    fn parquet_path_constructs_correctly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");
        std::fs::write(tmp.path().join("stock_daily/000001.parquet"), b"").expect("write");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        assert!(reader.file_exists("000001"));
        assert!(!reader.file_exists("999999"));
    }

    #[test]
    fn sql_injection_via_symbol_is_blocked() {
        let (_tmp, reader) = create_test_parquet_dir();

        // Malicious symbol that would cause SQL injection if not parameterized
        let malicious = "' OR 1=1 --";
        let result = reader.get_stock_basic_blocking(malicious);
        assert!(
            result.is_err() || result.unwrap().is_none(),
            "SQL injection via symbol should return error or None, not data"
        );
    }

    #[test]
    fn path_traversal_via_symbol_is_blocked() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        // Symbol with path traversal should not read files outside stock_daily/
        let malicious = "../../etc/passwd";
        assert!(
            !reader.file_exists(malicious),
            "path traversal symbol should not match any file"
        );
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

    /// Create a temp Parquet dataset with stock_daily data for 2 symbols.
    fn create_test_ohlcv_parquet(tmp: &tempfile::TempDir, symbol: &str, rows: &[(&str, f64)]) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t (tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
        ).expect("create");
        for (date, close) in rows {
            conn.execute(
                "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    date,
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
        let path = tmp
            .path()
            .join("stock_daily")
            .join(format!("{symbol}.parquet"));
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy");
    }

    #[test]
    fn fetch_bars_returns_sorted_ohlcv() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");
        create_test_ohlcv_parquet(
            &tmp,
            "000001",
            &[
                ("2024-01-02", 10.0),
                ("2024-01-03", 11.0),
                ("2024-01-04", 10.5),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let start = DateTime::from_timestamp(0, 0).unwrap();
        let end = DateTime::from_timestamp(4_000_000_000, 0).unwrap();
        let bars = reader
            .fetch_bars_blocking("000001", start, end)
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
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");
        create_test_ohlcv_parquet(
            &tmp,
            "000001",
            &[
                ("2024-01-02", 10.0),
                ("2024-01-03", 11.0),
                ("2024-02-01", 12.0),
            ],
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
            .fetch_bars_blocking("000001", start, end)
            .expect("fetch");
        assert_eq!(bars.len(), 2, "should only return Jan dates");
    }

    #[test]
    fn get_stored_range_returns_min_max() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");
        create_test_ohlcv_parquet(
            &tmp,
            "600519",
            &[
                ("2024-06-01", 1500.0),
                ("2024-06-15", 1510.0),
                ("2024-06-30", 1520.0),
            ],
        );

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let range = reader
            .get_stored_range("600519")
            .expect("range")
            .expect("some");
        assert_eq!(range.0.to_string(), "2024-06-01");
        assert_eq!(range.1.to_string(), "2024-06-30");
    }

    #[test]
    fn list_symbols_finds_parquet_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("stock_daily")).expect("mkdir");
        create_test_ohlcv_parquet(&tmp, "000001", &[("2024-01-01", 10.0)]);
        create_test_ohlcv_parquet(&tmp, "600519", &[("2024-01-01", 1500.0)]);

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        let symbols = reader.list_symbols().expect("list");
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].code, "000001");
        assert_eq!(symbols[1].code, "600519");
    }
}
