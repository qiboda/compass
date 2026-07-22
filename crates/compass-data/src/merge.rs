use std::path::PathBuf;

use duckdb::{Connection, params};
use tracing::{error, info, warn};

/// Merge staging DuckDB into Parquet main database with incremental updates.
pub async fn run(db: PathBuf, output: PathBuf) {
    info!(
        "merge: staging={} → parquet={}",
        db.display(),
        output.display()
    );

    let conn = match Connection::open(db.to_str().unwrap_or("staging.duckdb")) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open staging DB: {e}");
            return;
        }
    };

    std::fs::create_dir_all(output.join("stock_daily")).ok();

    let symbols: Vec<String> = match get_staging_symbols(&conn) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to list staging symbols: {e}");
            return;
        }
    };

    if symbols.is_empty() {
        warn!("Staging DB is empty — nothing to merge.");
        return;
    }

    let mut merged_new = 0usize;
    let mut merged_update = 0usize;
    let mut skipped = 0usize;

    for symbol in &symbols {
        let parquet_path = output.join("stock_daily").join(format!("{symbol}.parquet"));

        if !parquet_path.exists() {
            if merge_full(&conn, symbol, &parquet_path) {
                merged_new += 1;
            }
        } else if staging_has_new_data(&conn, symbol, &parquet_path) {
            if merge_incremental(&conn, symbol, &parquet_path) {
                merged_update += 1;
            }
        } else {
            skipped += 1;
        }
    }

    info!(
        "Merge done: {} new, {} updated, {} skipped, {} total",
        merged_new,
        merged_update,
        skipped,
        symbols.len()
    );
}

fn get_staging_symbols(conn: &Connection) -> Result<Vec<String>, duckdb::Error> {
    let mut stmt = conn.prepare("SELECT DISTINCT symbol FROM stock_daily ORDER BY symbol")?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn staging_date_range(conn: &Connection, symbol: &str) -> Option<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT MIN(CAST(trade_date AS VARCHAR)), MAX(CAST(trade_date AS VARCHAR)) FROM stock_daily WHERE symbol = ?")
        .ok()?;
    let (min, max) = stmt
        .query_row(params![symbol], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .ok()?;
    Some((min?, max?))
}

fn parquet_date_range(path: &std::path::Path) -> Option<(String, String)> {
    let conn = Connection::open_in_memory().ok()?;
    let sql = format!(
        "SELECT CAST(MIN(tradedate) AS VARCHAR), CAST(MAX(tradedate) AS VARCHAR) FROM read_parquet('{}')",
        path.to_string_lossy()
    );
    let mut stmt = conn.prepare(&sql).ok()?;
    let (min, max) = stmt
        .query_row([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .ok()?;
    Some((min?, max?))
}

fn staging_has_new_data(conn: &Connection, symbol: &str, path: &std::path::Path) -> bool {
    let staging = staging_date_range(conn, symbol);
    let parquet = parquet_date_range(path);
    match (staging, parquet) {
        (Some((s_min, s_max)), Some((p_min, p_max))) => s_min < p_min || s_max > p_max,
        (Some(_), None) => true, // Parquet unreadable → assume new data
        _ => false,
    }
}

fn merge_full(conn: &Connection, symbol: &str, parquet_path: &std::path::Path) -> bool {
    let sql = format!(
        "COPY (SELECT trade_date AS tradedate, open, high, low, close, adjclose, volume, amount FROM stock_daily WHERE symbol = ? ORDER BY trade_date) \
         TO '{}' (FORMAT PARQUET)",
        parquet_path.display()
    );
    match conn.execute(&sql, params![symbol]) {
        Ok(_) => true,
        Err(e) => {
            error!("merge failed for {symbol}: {e}");
            false
        }
    }
}

fn merge_incremental(conn: &Connection, symbol: &str, parquet_path: &std::path::Path) -> bool {
    let pq_path = parquet_path.to_string_lossy();
    let tmp_path = parquet_path.with_extension("tmp.parquet");
    let tmp_str = tmp_path.to_string_lossy();

    let sql = format!(
        "COPY (
            SELECT tradedate, open, high, low, close, adjclose, volume, amount
            FROM (
                SELECT *, ROW_NUMBER() OVER (PARTITION BY tradedate ORDER BY priority) AS rn
                FROM (
                    SELECT trade_date AS tradedate, open, high, low, close, adjclose, volume, amount, 1 AS priority
                    FROM stock_daily WHERE symbol = ?
                    UNION ALL
                    SELECT tradedate, open, high, low, close, adjclose, volume, amount, 2
                    FROM read_parquet('{pq_path}')
                )
            ) WHERE rn = 1
            ORDER BY tradedate
        ) TO '{tmp_str}' (FORMAT PARQUET)"
    );

    match conn.execute(&sql, params![symbol]) {
        Ok(_) => {
            match std::fs::copy(&tmp_path, parquet_path) {
                Ok(_) => {
                    let _ = std::fs::remove_file(&tmp_path);
                    true
                }
                Err(e) => {
                    error!("copy temp parquet for {symbol}: {e}");
                    let _ = std::fs::remove_file(&tmp_path);
                    false
                }
            }
        }
        Err(e) => {
            error!("incremental merge for {symbol}: {e}");
            let _ = std::fs::remove_file(&tmp_path);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::params;
    use tempfile::TempDir;

    // Initialize tracing for test debugging
    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("error")
            .try_init();
    }

    fn create_staging() -> (TempDir, Connection) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let conn = Connection::open(tmp.path().join("staging.duckdb")).expect("open");
        conn.execute_batch(
            "CREATE TABLE stock_daily (
                symbol VARCHAR NOT NULL, trade_date DATE NOT NULL,
                open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE,
                adjclose DOUBLE, volume DOUBLE, amount DOUBLE,
                PRIMARY KEY (symbol, trade_date))",
        ).expect("create");
        (tmp, conn)
    }

    fn insert_row(conn: &Connection, symbol: &str, date: &str, close: f64) {
        conn.execute(
            "INSERT OR REPLACE INTO stock_daily VALUES (?, ?, 1,2,1,?,?,100,0)",
            params![symbol, date, close, close],
        ).expect("insert");
    }

    #[test]
    fn date_range_returns_correct_bounds() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "000001", "2024-01-02", 10.0);
        insert_row(&conn, "000001", "2024-01-05", 12.0);
        let r = staging_date_range(&conn, "000001").expect("range");
        assert_eq!(r, ("2024-01-02".into(), "2024-01-05".into()));
    }

    #[test]
    fn date_range_none_for_missing() {
        let (_tmp, conn) = create_staging();
        assert!(staging_date_range(&conn, "999999").is_none());
    }

    #[test]
    fn full_export_creates_parquet() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "600519", "2024-06-01", 1500.0);
        insert_row(&conn, "600519", "2024-06-02", 1510.0);
        let out = tempfile::tempdir().expect("tempdir");
        let path = out.path().join("600519.parquet");
        assert!(merge_full(&conn, "600519", &path));
        let v = Connection::open_in_memory().expect("duckdb");
        let n: i64 = v.query_row(
            &format!("SELECT COUNT(*) FROM read_parquet('{}')", path.display()),
            [], |r| r.get(0),
        ).expect("query");
        assert_eq!(n, 2);
    }

    #[test]
    fn detects_new_data_when_staging_has_later_date() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "000001", "2024-01-01", 10.0);
        insert_row(&conn, "000001", "2024-12-31", 20.0);
        let out = tempfile::tempdir().expect("tempdir");
        let path = out.path().join("000001.parquet");
        merge_full(&conn, "000001", &path);
        insert_row(&conn, "000001", "2025-01-01", 25.0);
        assert!(staging_has_new_data(&conn, "000001", &path));
    }

    #[test]
    fn no_new_data_when_fully_covered() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "000001", "2024-01-01", 10.0);
        insert_row(&conn, "000001", "2024-12-31", 20.0);
        let out = tempfile::tempdir().expect("tempdir");
        let path = out.path().join("000001.parquet");
        merge_full(&conn, "000001", &path);
        conn.execute("DELETE FROM stock_daily WHERE symbol = '000001'", []).expect("del");
        insert_row(&conn, "000001", "2024-06-15", 15.0);
        assert!(!staging_has_new_data(&conn, "000001", &path));
    }

    #[test]
    fn incremental_merge_adds_new_date_and_prefers_staging() {
        init_tracing();
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "600519", "2024-01-02", 1500.0);
        insert_row(&conn, "600519", "2024-01-03", 1510.0);
        let out = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(out.path().join("stock_daily")).expect("mkdir");
        let path = out.path().join("stock_daily").join("600519.parquet");
        merge_full(&conn, "600519", &path);

        insert_row(&conn, "600519", "2024-01-03", 1515.0);
        insert_row(&conn, "600519", "2024-01-04", 1520.0);

        assert!(staging_has_new_data(&conn, "600519", &path));
        assert!(merge_incremental(&conn, "600519", &path));

        let v = Connection::open_in_memory().expect("duckdb");
        let mut s = v.prepare(
            &format!("SELECT CAST(tradedate AS VARCHAR), close FROM read_parquet('{}') ORDER BY tradedate", path.display()),
        ).expect("prep");
        let rows: Vec<(String, f64)> = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("q").collect::<Result<Vec<_>,_>>().expect("c");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], ("2024-01-02".into(), 1500.0));
        assert_eq!(rows[1], ("2024-01-03".into(), 1515.0));
        assert_eq!(rows[2], ("2024-01-04".into(), 1520.0));
    }

    #[test]
    fn get_staging_symbols_returns_sorted() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "600519", "2024-01-01", 10.0);
        insert_row(&conn, "000001", "2024-01-01", 10.0);
        let symbols = get_staging_symbols(&conn).expect("symbols");
        assert_eq!(symbols, vec!["000001".to_string(), "600519".to_string()]);
    }

    #[test]
    fn staging_date_range_ignores_other_symbols() {
        let (_tmp, conn) = create_staging();
        insert_row(&conn, "000001", "2024-03-01", 10.0);
        insert_row(&conn, "600519", "2024-01-01", 1500.0);
        insert_row(&conn, "600519", "2024-12-31", 1510.0);
        let r = staging_date_range(&conn, "600519").expect("range");
        assert_eq!(r, ("2024-01-01".into(), "2024-12-31".into()));
    }

    #[test]
    fn parquet_date_range_returns_none_for_missing_file() {
        assert!(parquet_date_range(std::path::Path::new("/nonexistent/000001.parquet")).is_none());
    }
}
