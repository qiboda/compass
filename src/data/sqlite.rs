use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use rusqlite::{Connection, params};

use crate::data::provider::{DataError, DataProvider, DataWriter};
use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// SqliteProvider — local persistent cache
// ---------------------------------------------------------------------------

pub struct SqliteProvider {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteProvider {
    /// Open (or create) the SQLite database at `path` and ensure the schema exists.
    pub fn new(path: &str) -> Result<Self, DataError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bars (
                symbol      TEXT NOT NULL,
                timeframe   TEXT NOT NULL,
                timestamp   INTEGER NOT NULL,
                open        REAL,
                high        REAL,
                low         REAL,
                close       REAL,
                volume      REAL,
                adj_type    TEXT NOT NULL DEFAULT 'none',
                adj_factor  REAL,
                status      TEXT NOT NULL DEFAULT 'normal',
                PRIMARY KEY (symbol, timeframe, adj_type, timestamp)
            );
            CREATE INDEX IF NOT EXISTS idx_bars_lookup
                ON bars(symbol, timeframe, adj_type, timestamp DESC);",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

// ---------------------------------------------------------------------------
// DataProvider — read-only data source
// ---------------------------------------------------------------------------

#[async_trait]
impl DataProvider for SqliteProvider {
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        let symbol = symbol.to_string();
        let timeframe = timeframe.to_string();
        let start_ts = range_start.timestamp();
        let end_ts = range_end.timestamp();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "SELECT timestamp, open, high, low, close, volume
                     FROM bars
                     WHERE symbol = ?1 AND timeframe = ?2
                       AND timestamp >= ?3 AND timestamp <= ?4
                     ORDER BY timestamp ASC",
                )
                .map_err(DataError::from)?;

            let rows = stmt
                .query_map(params![symbol, timeframe, start_ts, end_ts], |row| {
                    let ts: i64 = row.get(0)?;
                    Ok(Bar {
                        time: DateTime::from_timestamp(ts, 0).unwrap_or_default(),
                        open: row.get(1)?,
                        high: row.get(2)?,
                        low: row.get(3)?,
                        close: row.get(4)?,
                        volume: row.get(5)?,
                    })
                })
                .map_err(DataError::from)?;

            rows.collect::<Result<Vec<Bar>, rusqlite::Error>>()
                .map_err(DataError::from)
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }

    async fn search_symbols(&self, _query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        // SQLite provider does not store symbol metadata.
        // Symbol search is handled by the remote provider (e.g. EastMoney).
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// DataWriter — write-through cache interface
// ---------------------------------------------------------------------------

#[async_trait]
impl DataWriter for SqliteProvider {
    async fn save_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        bars: &[Bar],
    ) -> Result<(), DataError> {
        if bars.is_empty() {
            return Ok(());
        }

        // Extract fields into owned values so they can be moved into spawn_blocking.
        let symbol = symbol.to_string();
        let timeframe = timeframe.to_string();
        let records: Vec<(i64, f64, f64, f64, f64, f64)> = bars
            .iter()
            .map(|b| (b.time.timestamp(), b.open, b.high, b.low, b.close, b.volume))
            .collect();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || -> Result<(), DataError> {
            let conn = conn.lock().map_err(|e| DataError::Parse(format!("mutex poisoned: {e}")))?;
            let mut stmt = conn
                .prepare(
                    "INSERT OR REPLACE INTO bars
                        (symbol, timeframe, timestamp, open, high, low, close, volume, adj_type, adj_factor, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'none', 0.0, 'normal')",
                )
                .map_err(DataError::from)?;

            for (ts, open, high, low, close, volume) in &records {
                stmt.execute(params![symbol, timeframe, ts, open, high, low, close, volume])
                    .map_err(DataError::from)?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DataError::Parse(format!("spawn_blocking panicked: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::provider::{DataProvider, DataWriter};
    use chrono::Utc;
    use rstest::rstest;

    fn make_bar(day: u32, open: f64, close: f64, volume: f64) -> Bar {
        Bar {
            time: Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
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
        let provider = SqliteProvider::new(":memory:").unwrap();

        let bars = vec![
            make_bar(1, 10.0, 10.5, 1000.0),
            make_bar(2, 10.5, 11.0, 2000.0),
        ];

        provider.save_bars(symbol, timeframe, &bars).await.unwrap();

        let fetched = provider
            .fetch_bars(symbol, timeframe, fetch_all_start(), fetch_all_end())
            .await
            .unwrap();

        assert_eq!(fetched.len(), 2, "wrong count for {symbol}/{timeframe}");
        assert_eq!(fetched[0].open, 10.0);
        assert_eq!(fetched[0].close, 10.5);
        assert_eq!(fetched[1].open, 10.5);
        assert_eq!(fetched[1].close, 11.0);

        let other_sym = provider
            .fetch_bars("NOT_EXIST", timeframe, fetch_all_start(), fetch_all_end())
            .await
            .unwrap();
        assert!(
            other_sym.is_empty(),
            "should have no data for different symbol"
        );

        let other_tf = provider
            .fetch_bars(symbol, "999d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();
        assert!(
            other_tf.is_empty(),
            "should have no data for different timeframe"
        );
    }

    fn fetch_all_start() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(0, 0).unwrap()
    }

    fn fetch_all_end() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(4_000_000_000, 0).unwrap()
    }
}
