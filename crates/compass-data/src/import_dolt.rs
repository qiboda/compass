use std::path::{Path, PathBuf};

use duckdb::Connection;
use indicatif::{ProgressBar, ProgressStyle};
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
fn run_dolt_sql_csv(dolt_dir: &Path, query: &str) -> Result<String, String> {
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
fn run_dolt_sql_parquet(dolt_dir: &Path, query: &str) -> Result<Vec<u8>, String> {
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

/// Parquet file size threshold: files smaller than this are considered empty
/// (schema-only, 0 data rows). A single OHLCV row is ~200 bytes.
const MIN_PARQUET_SIZE: u64 = 500;

/// Filter symbols by 6-digit codes. `filter` is comma-separated (e.g. "000001,600519").
/// Matches against full Dolt symbols (e.g. "SZ000001", "SH600519") by stripping prefix.
fn filter_symbols(symbols: Vec<String>, filter: &str) -> Vec<String> {
    let wanted: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
    symbols
        .into_iter()
        .filter(|s| wanted.iter().any(|w| strip_prefix(s) == *w))
        .collect()
}

pub fn run(
    dolt_dir: PathBuf,
    output: PathBuf,
    limit: usize,
    symbols_filter: Option<&str>,
    start_date: Option<&str>,
    end_date: Option<&str>,
    overwrite: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&output)?;

    // ------------------------------------------------------------------
    // 1. Get distinct symbols
    // ------------------------------------------------------------------
    info!("Fetching symbol list...");
    let symbols_csv = run_dolt_sql_csv(
        &dolt_dir,
        "SELECT DISTINCT symbol FROM final_a_stock_eod_price ORDER BY symbol",
    )?;

    let symbols: Vec<String> = symbols_csv
        .lines()
        .skip(1) // skip header
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    // Filter by requested symbols (6-digit codes)
    let symbols = if let Some(filter) = symbols_filter {
        filter_symbols(symbols, filter)
    } else {
        symbols
    };

    let total = if limit > 0 {
        symbols.len().min(limit)
    } else {
        symbols.len()
    };
    let date_filter = match (start_date, end_date) {
        (Some(s), Some(e)) => format!("AND tradedate >= '{s}' AND tradedate <= '{e}'"),
        (Some(s), None) => format!("AND tradedate >= '{s}'"),
        (None, Some(e)) => format!("AND tradedate <= '{e}'"),
        (None, None) => String::new(),
    };

    if start_date.is_some() || end_date.is_some() {
        info!(
            "Date filter: {}..={}",
            start_date.unwrap_or("min"),
            end_date.unwrap_or("max")
        );
    }
    info!("Found {} symbols, exporting {}...", symbols.len(), total);

    // ------------------------------------------------------------------
    // 2. Export stock_basic — direct Parquet from Dolt
    // ------------------------------------------------------------------
    info!("Exporting stock_basic...");
    let basic_bytes = run_dolt_sql_parquet(
        &dolt_dir,
        "SELECT symbol, '' AS name, \
         CASE WHEN exchange = 'SZSE' THEN 'SZ' WHEN exchange = 'SHSE' THEN 'SH' ELSE exchange END AS exchange, \
         list_date, delist_date FROM ts_a_stock_list",
    )?;

    let basic_path = output.join("stock_basic.parquet");
    std::fs::write(&basic_path, &basic_bytes)?;
    info!("  → {}/stock_basic.parquet", output.display());

    // ------------------------------------------------------------------
    // 3. Export stock_daily — one Parquet file per symbol
    // ------------------------------------------------------------------
    info!("Exporting stock_daily ({} symbols)...", total);
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner} [{elapsed_precise}] [{bar:40}] {pos}/{len} {msg}")
            .unwrap(),
    );

    let work_dir = std::env::temp_dir().join("compass_parquet_work");
    std::fs::create_dir_all(&work_dir)?;

    let stock_daily_dir = output.join("stock_daily");
    std::fs::create_dir_all(&stock_daily_dir)?;

    // DuckDB connection for merge operations (overwrite=false + existing file)
    let duck = Connection::open_in_memory()?;

    for (i, symbol) in symbols.iter().take(total).enumerate() {
        let code = strip_prefix(symbol);
        pb.set_message(code.to_string());

        let query = format!(
            "SELECT tradedate, open, high, low, close, adjclose, volume, amount \
             FROM final_a_stock_eod_price \
             WHERE symbol = '{symbol}' {date_filter} \
             ORDER BY tradedate"
        );
        let parquet_data = match run_dolt_sql_parquet(&dolt_dir, &query) {
            Ok(data) => data,
            Err(e) => {
                warn!("dolt query failed for {symbol}: {e}");
                pb.inc(1);
                continue;
            }
        };

        // Empty Parquet (schema only, 0 rows) is ~219 bytes. Skip.
        let len = parquet_data.len() as u64;
        if len < MIN_PARQUET_SIZE {
            warn!("symbol {symbol} returned empty data ({len} bytes), skipping");
            pb.inc(1);
            continue;
        }

        let parquet_path = stock_daily_dir.join(format!("{symbol}.parquet"));

        if !overwrite && parquet_path.exists() {
            // Merge: keep existing data (priority 1), only add new dates from Dolt (priority 2)
            let new_path = work_dir.join(format!("{code}.new.parquet"));
            std::fs::write(&new_path, &parquet_data)?;

            let tmp_path = work_dir.join(format!("{code}.tmp.parquet"));
            let sql = format!(
                "COPY (
                    SELECT tradedate, open, high, low, close, adjclose, volume, amount
                    FROM (
                        SELECT *, ROW_NUMBER() OVER (PARTITION BY tradedate ORDER BY priority) AS rn
                        FROM (
                            SELECT tradedate, open, high, low, close, adjclose, volume, amount, 1 AS priority
                            FROM read_parquet('{}')
                            UNION ALL
                            SELECT tradedate, open, high, low, close, adjclose, volume, amount, 2
                            FROM read_parquet('{}')
                        )
                    ) WHERE rn = 1
                    ORDER BY tradedate
                ) TO '{}' (FORMAT PARQUET)",
                parquet_path.display(),
                new_path.display(),
                tmp_path.display(),
            );
            if let Err(e) = duck.execute_batch(&sql) {
                warn!("DuckDB merge failed for {code}: {e}");
            } else {
                if let Err(e) = std::fs::copy(&tmp_path, &parquet_path) {
                    warn!("copy merged parquet for {code}: {e}");
                }
            }
            let _ = std::fs::remove_file(&new_path);
            let _ = std::fs::remove_file(&tmp_path);
        } else {
            // New file or overwrite: write Parquet bytes directly
            if let Err(e) = std::fs::write(&parquet_path, &parquet_data) {
                warn!("write parquet failed for {code}: {e}");
            }
        }

        pb.inc(1);

        if (i + 1) % 100 == 0 || i + 1 == total {
            info!(
                "Progress: {}/{} symbols ({:.1}%)",
                i + 1,
                total,
                (i + 1) as f64 / total as f64 * 100.0
            );
        }
    }

    // Remove temp work directory if empty
    let _ = std::fs::remove_dir(&work_dir);

    pb.finish_with_message("Done!");

    // ------------------------------------------------------------------
    // 4. Summary
    // ------------------------------------------------------------------
    let file_count = std::fs::read_dir(&stock_daily_dir)
        .map(|d| d.count())
        .unwrap_or(0);

    info!("==============================");
    info!(
        "Exported {} Parquet files to {}/stock_daily/",
        file_count,
        output.display()
    );
    info!("==============================");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[test]
    fn run_dolt_sql_parquet_returns_small_file_for_nonexistent_symbol() {
        let dolt_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../investment_data");
        let result = run_dolt_sql_parquet(
            &dolt_dir,
            "SELECT tradedate, open, high, low, close, adjclose, volume, amount \
             FROM final_a_stock_eod_price \
             WHERE symbol = 'SZ999999' ORDER BY tradedate",
        );
        assert!(result.is_ok());
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
}
