use std::path::{Path, PathBuf};

use tracing::{info, warn};

/// Strip SH/SZ/BJ prefix from symbol, returning the 6-digit code.
fn strip_prefix(symbol: &str) -> &str {
    if let Some(rest) = symbol.strip_prefix("SH") {
        rest
    } else if let Some(rest) = symbol.strip_prefix("SZ") {
        rest
    } else if let Some(rest) = symbol.strip_prefix("BJ") {
        rest
    } else {
        symbol
    }
}

/// Run `dolt sql -r csv` and return the CSV output as a string.
pub fn run_dolt_sql_csv(dolt_dir: &Path, query: &str) -> Result<String, String> {
    let output = std::process::Command::new("dolt")
        .arg("--data-dir")
        .arg(dolt_dir)
        .arg("sql")
        .arg("-r")
        .arg("csv")
        .arg("-q")
        .arg(query)
        .output()
        .map_err(|e| format!("dolt command failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dolt error: {stderr}"));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("UTF-8 error: {e}"))
}

/// Run `dolt sql -r parquet` and return the binary Parquet output.
pub fn run_dolt_sql_parquet(dolt_dir: &Path, query: &str) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("dolt")
        .arg("--data-dir")
        .arg(dolt_dir)
        .arg("sql")
        .arg("-r")
        .arg("parquet")
        .arg("-q")
        .arg(query)
        .output()
        .map_err(|e| format!("dolt command failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dolt error: {stderr}"));
    }

    Ok(output.stdout)
}

/// Filter symbols by 6-digit codes. `filter` is comma-separated (e.g. "000001,600519").
/// Matches against full Dolt symbols (e.g. "SZ000001", "SH600519") by stripping prefix.
fn filter_symbols(symbols: Vec<String>, filter: &str) -> Vec<String> {
    let wanted: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
    symbols
        .into_iter()
        .filter(|s| wanted.iter().any(|w| strip_prefix(s) == *w))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    dolt_dir: PathBuf,
    output: PathBuf,
    limit: usize,
    symbols_filter: Option<&str>,
    start_date: Option<&str>,
    end_date: Option<&str>,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&output)?;

    // ------------------------------------------------------------------
    // 1. Migration detection: warn if legacy stock_daily/ directory exists
    // ------------------------------------------------------------------
    let legacy_dir = output.join("stock_daily");
    if legacy_dir.exists() && legacy_dir.is_dir() {
        warn!(
            "Found legacy per-symbol files at {}/stock_daily/ — run `import` to regenerate single-file format, then remove stock_daily/",
            output.display()
        );
    }

    // ------------------------------------------------------------------
    // 2. Get distinct symbols (for summary / symbols.txt)
    //    --since no longer affects symbol enumeration — it is only a
    //    WHERE filter on the data query below.
    // ------------------------------------------------------------------
    info!("Fetching symbol list...");
    let symbol_query =
        "SELECT DISTINCT symbol FROM final_a_stock_eod_price ORDER BY symbol".to_string();
    let symbols_csv = run_dolt_sql_csv(&dolt_dir, &symbol_query)?;

    let symbols: Vec<String> = symbols_csv
        .lines()
        .skip(1) // skip header
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    let symbols = if let Some(filter) = symbols_filter {
        filter_symbols(symbols, filter)
    } else {
        symbols
    };

    info!(
        "Found {} symbols, exporting all daily data...",
        symbols.len()
    );

    // ------------------------------------------------------------------
    // 3. Export stock_basic — direct Parquet from Dolt (unchanged)
    // ------------------------------------------------------------------
    info!("Exporting stock_basic...");
    let basic_bytes = run_dolt_sql_parquet(
        &dolt_dir,
        "SELECT symbol, symbol AS name, \
         CASE WHEN exchange = 'SZSE' THEN 'SZ' WHEN exchange = 'SHSE' THEN 'SH' ELSE exchange END AS exchange, \
         list_date, delist_date FROM ts_a_stock_list",
    )?;

    let basic_path = output.join("stock_basic.parquet");
    std::fs::write(&basic_path, &basic_bytes)?;
    info!("  → {}/stock_basic.parquet", output.display());

    // ------------------------------------------------------------------
    // 4. Export stock_daily — single Parquet file with symbol column
    //    Builds one SQL query with all filters, writes a single file,
    //    and generates a companion symbols.txt.
    // ------------------------------------------------------------------
    info!("Exporting stock_daily to single parquet file...");

    // Build WHERE clause from all filters
    let mut where_parts: Vec<String> = Vec::new();

    // --since: tradedate filter (does NOT affect symbol enumeration)
    if let Some(since_date) = since {
        if since_date.len() != 8 || !since_date.chars().all(|c| c.is_ascii_digit()) {
            return Err("--since must be YYYYMMDD (8 digits)".into());
        }
        where_parts.push(format!("tradedate >= '{since_date}'"));
    }

    // --symbols: filter by 6-digit code (strip prefix for comparison)
    if let Some(filter) = symbols_filter {
        let codes: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        let quoted: Vec<String> = codes.iter().map(|c| format!("'{c}'")).collect();
        where_parts.push(format!(
            "CASE WHEN LEFT(symbol,2) IN ('SH','SZ','BJ') THEN SUBSTRING(symbol,3) ELSE symbol END IN ({})",
            quoted.join(",")
        ));
    }

    // --start-date / --end-date
    match (start_date, end_date) {
        (Some(s), Some(e)) => {
            where_parts.push(format!("tradedate >= '{s}' AND tradedate <= '{e}'"));
            info!("Date filter: {s}..={e}");
        }
        (Some(s), None) => {
            where_parts.push(format!("tradedate >= '{s}'"));
            info!("Date filter: {s}..=max");
        }
        (None, Some(e)) => {
            where_parts.push(format!("tradedate <= '{e}'"));
            info!("Date filter: min..={e}");
        }
        (None, None) => {}
    }

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_parts.join(" AND "))
    };

    let limit_clause = if limit > 0 {
        format!("LIMIT {limit}")
    } else {
        String::new()
    };

    let query = format!(
        "SELECT CASE WHEN LEFT(symbol,2) IN ('SH','SZ','BJ') THEN SUBSTRING(symbol,3) ELSE symbol END AS symbol, \
         tradedate, open, high, low, close, adjclose, volume, amount \
         FROM final_a_stock_eod_price \
         {where_clause} \
         ORDER BY symbol, tradedate \
         {limit_clause}"
    );

    info!("Running dolt query...");
    let daily_bytes = run_dolt_sql_parquet(&dolt_dir, &query)?;

    // Write to temp file first, then atomic rename
    let tmp_path = output.join("stock_daily.tmp.parquet");
    let final_path = output.join("stock_daily.parquet");
    std::fs::write(&tmp_path, &daily_bytes)?;
    std::fs::rename(&tmp_path, &final_path)?;

    // ------------------------------------------------------------------
    // 5. Generate symbols.txt (strip prefixes, sorted alphabetically)
    // ------------------------------------------------------------------
    let symbols_txt_path = output.join("stock_daily.symbols.txt");
    let mut sorted_codes: Vec<&str> = symbols.iter().map(|s| strip_prefix(s)).collect();
    sorted_codes.sort();
    std::fs::write(&symbols_txt_path, sorted_codes.join("\n"))?;

    // ------------------------------------------------------------------
    // 6. Get row count for summary
    // ------------------------------------------------------------------
    let count_query = if where_clause.is_empty() {
        "SELECT COUNT(*) AS cnt FROM final_a_stock_eod_price".to_string()
    } else {
        format!("SELECT COUNT(*) AS cnt FROM final_a_stock_eod_price {where_clause}")
    };
    let count_csv = run_dolt_sql_csv(&dolt_dir, &count_query)?;
    let row_count = count_csv
        .lines()
        .nth(1)
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(0);

    info!("==============================");
    info!(
        "Exported stock_daily.parquet ({} symbols, {} rows) with symbols index",
        symbols.len(),
        row_count,
    );
    info!("==============================");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Parquet file size threshold: files smaller than this are considered empty
    /// (schema-only, 0 data rows). A single OHLCV row is ~200 bytes.
    const MIN_PARQUET_SIZE: u64 = 500;

    const EOD_SCHEMA: &str = "CREATE TABLE final_a_stock_eod_price (\
         symbol VARCHAR(20) NOT NULL, \
         tradedate DATE NOT NULL, \
         open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, \
         adjclose DOUBLE, volume DOUBLE, amount DOUBLE, \
         PRIMARY KEY (symbol, tradedate))";

    const STOCK_LIST_SCHEMA: &str = "CREATE TABLE ts_a_stock_list (\
         symbol VARCHAR(20) PRIMARY KEY, \
         name VARCHAR(100), \
         exchange VARCHAR(10), \
         list_date DATE, \
         delist_date DATE)";

    fn setup_dolt(dir: &std::path::Path) {
        for (key, val) in [("user.email", "test@compass.local"), ("user.name", "Test")] {
            let out = Command::new("dolt")
                .arg("config")
                .arg("--global")
                .arg("--add")
                .arg(key)
                .arg(val)
                .output()
                .expect("dolt config");
            assert!(
                out.status.success(),
                "dolt config {key} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
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

    fn dolt_sql(dolt_dir: &std::path::Path, sql: &str) {
        let out = Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_dir)
            .arg("sql")
            .arg("-q")
            .arg(sql)
            .output()
            .expect("dolt sql");
        assert!(
            out.status.success(),
            "dolt sql failed: {}\nsql: {sql}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn dolt_setup_tables(dolt_dir: &std::path::Path) {
        dolt_sql(dolt_dir, EOD_SCHEMA);
        dolt_sql(dolt_dir, STOCK_LIST_SCHEMA);
        dolt_sql(
            dolt_dir,
            "INSERT INTO ts_a_stock_list VALUES \
             ('000001', '平安银行', 'SZSE', '1991-04-03', NULL), \
             ('600519', '贵州茅台', 'SHSE', '2001-08-27', NULL)",
        );
    }

    #[test]
    fn strip_prefix_removes_sh() {
        assert_eq!(strip_prefix("SH600519"), "600519");
        assert_eq!(strip_prefix("SH688001"), "688001");
    }

    #[test]
    fn strip_prefix_removes_sz() {
        assert_eq!(strip_prefix("SZ000001"), "000001");
        assert_eq!(strip_prefix("SZ300750"), "300750");
    }

    #[test]
    fn strip_prefix_removes_bj() {
        assert_eq!(strip_prefix("BJ830799"), "830799");
    }

    #[test]
    fn strip_prefix_passthrough_unknown() {
        assert_eq!(strip_prefix("000001"), "000001");
        assert_eq!(strip_prefix("600519"), "600519");
        assert_eq!(strip_prefix(""), "");
    }

    #[test]
    fn run_dolt_sql_csv_returns_error_for_nonexistent_dir() {
        let result = run_dolt_sql_csv(std::path::Path::new("/nonexistent/dolt/dir"), "SELECT 1");
        assert!(result.is_err());
    }

    /// When querying a symbol that doesn't exist in Dolt, the Parquet output
    /// is a valid file with schema but 0 data rows (~219 bytes, below MIN_PARQUET_SIZE).
    /// This triggers the skip path in the import loop.
    ///
    /// Uses a self-contained temp Dolt database — no dependency on the real
    /// `investment_data` repo. Requires `dolt` on PATH (installed in CI).
    #[test]
    fn run_dolt_sql_parquet_returns_small_file_for_nonexistent_symbol() {
        let tmp = tempfile::tempdir().expect("create temp dir");

        for (key, val) in [("user.email", "test@compass.local"), ("user.name", "Test")] {
            let out = std::process::Command::new("dolt")
                .arg("config")
                .arg("--global")
                .arg("--add")
                .arg(key)
                .arg(val)
                .output()
                .expect("dolt config");
            assert!(out.status.success(), "dolt config {key} failed");
        }

        let init = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        let create = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE final_a_stock_eod_price (\
                 symbol VARCHAR(20) NOT NULL, \
                 tradedate DATE NOT NULL, \
                 open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, \
                 adjclose DOUBLE, volume DOUBLE, amount DOUBLE, \
                 PRIMARY KEY (symbol, tradedate))",
            )
            .output()
            .expect("dolt sql create table");
        assert!(
            create.status.success(),
            "create table failed: {}",
            String::from_utf8_lossy(&create.stderr)
        );

        let result = run_dolt_sql_parquet(
            tmp.path(),
            "SELECT tradedate, open, high, low, close, adjclose, volume, amount \
             FROM final_a_stock_eod_price \
             WHERE symbol = 'SZ999999' ORDER BY tradedate",
        );
        assert!(result.is_ok(), "query failed: {:?}", result.err());
        let data = result.unwrap();
        assert!(
            (data.len() as u64) < MIN_PARQUET_SIZE,
            "nonexistent symbol should produce tiny Parquet ({} bytes < {}), got {} bytes",
            data.len(),
            MIN_PARQUET_SIZE,
            data.len()
        );
    }

    // ------------------------------------------------------------------
    // filter_symbols tests
    // ------------------------------------------------------------------

    #[test]
    fn filter_symbols_matches_sz_code() {
        let input = vec!["SZ000001".into(), "SH600519".into(), "SZ300750".into()];
        let result = filter_symbols(input, "000001");
        assert_eq!(result, vec!["SZ000001"]);
    }

    #[test]
    fn filter_symbols_matches_sh_code() {
        let input = vec!["SZ000001".into(), "SH600519".into()];
        let result = filter_symbols(input, "600519");
        assert_eq!(result, vec!["SH600519"]);
    }

    #[test]
    fn filter_symbols_matches_multiple_comma_separated() {
        let input = vec![
            "SZ000001".into(),
            "SH600519".into(),
            "SZ300750".into(),
            "BJ830799".into(),
        ];
        let result = filter_symbols(input, "000001,600519");
        assert_eq!(result, vec!["SZ000001", "SH600519"]);
    }

    #[test]
    fn filter_symbols_handles_spaces_in_filter() {
        let input = vec!["SZ000001".into(), "SH600519".into()];
        let result = filter_symbols(input, " 000001 , 600519 ");
        assert_eq!(result, vec!["SZ000001", "SH600519"]);
    }

    #[test]
    fn filter_symbols_returns_empty_on_no_match() {
        let input = vec!["SZ000001".into(), "SH600519".into()];
        let result = filter_symbols(input, "999999");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_symbols_returns_empty_on_empty_input() {
        let input: Vec<String> = vec![];
        let result = filter_symbols(input, "000001");
        assert!(result.is_empty());
    }

    #[test]
    fn run_dolt_sql_csv_success_returns_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE test (id INT PRIMARY KEY, name VARCHAR(50))",
        );
        dolt_sql(tmp.path(), "INSERT INTO test VALUES (1, 'hello')");

        let result = run_dolt_sql_csv(tmp.path(), "SELECT * FROM test ORDER BY id");
        assert!(result.is_ok(), "csv query failed: {:?}", result.err());
        let csv = result.unwrap();
        assert!(
            csv.contains("hello"),
            "csv should contain 'hello', got: {csv}"
        );
        assert!(csv.contains("1"), "csv should contain '1', got: {csv}");
    }

    #[test]
    fn run_rejects_invalid_since_not_8_digits() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        let result = run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            None,
            None,
            Some("2025"),
        );
        assert!(result.is_err(), "--since with 4 digits should be rejected");
    }

    #[test]
    fn run_rejects_invalid_since_non_digit() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        let result = run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            None,
            None,
            Some("2025010X"),
        );
        assert!(
            result.is_err(),
            "--since with non-digit chars should be rejected"
        );
    }

    #[test]
    fn run_filters_by_symbols() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SH600519', '2024-01-02', 99, 101, 98, 100, 100, 2000, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            Some("000001"),
            None,
            None,
            None,
        )
        .expect("run with --symbols");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());
        assert!(
            parquet.metadata().unwrap().len() > MIN_PARQUET_SIZE,
            "parquet should have data"
        );

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1, "should only contain 000001");

        let symbols: Vec<String> = duck
            .prepare(&format!(
                "SELECT DISTINCT symbol FROM read_parquet('{}') ORDER BY symbol",
                parquet.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(symbols, vec!["000001"]);
    }

    #[test]
    fn run_filters_by_start_date() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SZ000001', '2024-03-01', 12, 13, 11, 12.5, 12.5, 1500, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            Some("2024-03-01"),
            None,
            None,
        )
        .expect("run with --start-date");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(
            count, 1,
            "start-date filter should only include 1 row (2024-03-01)"
        );
    }

    #[test]
    fn run_filters_by_end_date() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SZ000001', '2024-03-01', 12, 13, 11, 12.5, 12.5, 1500, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            None,
            Some("2024-01-15"),
            None,
        )
        .expect("run with --end-date");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(
            count, 1,
            "end-date filter should only include 1 row (2024-01-02)"
        );
    }

    #[test]
    fn run_filters_by_date_range() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SZ000001', '2024-02-15', 10, 12, 9, 11, 11, 1200, 0), \
             ('SZ000001', '2024-03-01', 12, 13, 11, 12.5, 12.5, 1500, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            Some("2024-02-01"),
            Some("2024-02-28"),
            None,
        )
        .expect("run with date range");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1, "date range should only include the Feb 15 row");
    }

    #[test]
    fn run_respects_limit() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SZ000001', '2024-01-03', 10, 12, 9, 11, 11, 1200, 0), \
             ('SZ000001', '2024-01-04', 11, 13, 10, 12, 12, 1400, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            2,
            None,
            None,
            None,
            None,
        )
        .expect("run with --limit");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 2, "limit=2 should return exactly 2 rows");
    }

    #[test]
    fn run_valid_since_with_8_digits_exports_data() {
        let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
        setup_dolt(dolt_tmp.path());
        dolt_setup_tables(dolt_tmp.path());
        dolt_sql(
            dolt_tmp.path(),
            "INSERT INTO final_a_stock_eod_price VALUES \
             ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0), \
             ('SZ000001', '2024-03-01', 12, 13, 11, 12.5, 12.5, 1500, 0)",
        );

        let output_tmp = tempfile::tempdir().expect("output tmp");
        run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            None,
            None,
            Some("20240201"),
        )
        .expect("run with valid --since");

        let parquet = output_tmp.path().join("stock_daily.parquet");
        assert!(parquet.exists());
        assert!(
            parquet.metadata().unwrap().len() > MIN_PARQUET_SIZE,
            "parquet should have data after valid since filter"
        );

        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(
            count, 1,
            "--since 20240201 should only include 2024-03-01 row"
        );
    }

    /// When `stock_daily/` directory already exists in the output path,
    /// `run()` should still complete successfully (the directory is detected
    /// and a warning is logged via tracing, but import proceeds normally).
    #[test]
    fn run_completes_when_legacy_stock_daily_dir_exists() {
        let dolt_tmp = tempfile::tempdir().expect("create dolt temp dir");

        // Set up Dolt config
        for (key, val) in [("user.email", "test@compass.local"), ("user.name", "Test")] {
            let out = std::process::Command::new("dolt")
                .arg("config")
                .arg("--global")
                .arg("--add")
                .arg(key)
                .arg(val)
                .output()
                .expect("dolt config");
            assert!(out.status.success(), "dolt config {key} failed");
        }

        // Init Dolt repo
        let init = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_tmp.path())
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        // Create final_a_stock_eod_price table with sample data
        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE final_a_stock_eod_price (\
                 symbol VARCHAR(20) NOT NULL, \
                 tradedate DATE NOT NULL, \
                 open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, \
                 adjclose DOUBLE, volume DOUBLE, amount DOUBLE, \
                 PRIMARY KEY (symbol, tradedate))",
            )
            .output()
            .expect("dolt create table");
        assert!(
            out.status.success(),
            "create eod table failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO final_a_stock_eod_price VALUES ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0)")
            .output()
            .expect("dolt insert");
        assert!(
            out.status.success(),
            "insert failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Create ts_a_stock_list table for stock_basic export
        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE ts_a_stock_list (\
                 symbol VARCHAR(20) PRIMARY KEY, \
                 name VARCHAR(100), \
                 exchange VARCHAR(10), \
                 list_date DATE, \
                 delist_date DATE)",
            )
            .output()
            .expect("dolt create stock list");
        assert!(
            out.status.success(),
            "create stock list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let out = std::process::Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO ts_a_stock_list VALUES ('000001', '平安银行', 'SZSE', '1991-04-03', NULL)")
            .output()
            .expect("dolt insert stock");
        assert!(
            out.status.success(),
            "insert stock failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Create output dir with legacy stock_daily/ subdirectory
        let output_tmp = tempfile::tempdir().expect("create output dir");
        let legacy_dir = output_tmp.path().join("stock_daily");
        std::fs::create_dir(&legacy_dir).expect("create legacy dir");

        // Run import — should succeed despite legacy directory
        let result = run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            None,
            None,
            None,
            None,
        );
        assert!(
            result.is_ok(),
            "import should succeed with legacy dir: {:?}",
            result.err()
        );

        // Verify both output files were created
        assert!(
            output_tmp.path().join("stock_daily.parquet").exists(),
            "stock_daily.parquet should exist"
        );
        assert!(
            output_tmp.path().join("stock_basic.parquet").exists(),
            "stock_basic.parquet should exist"
        );
    }
}
