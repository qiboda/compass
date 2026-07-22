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

fn run_dolt_sql(dolt_dir: &Path, query: &str) -> Result<String, String> {
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

pub fn run(
    dolt_dir: PathBuf,
    output: PathBuf,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&output)?;

    // ------------------------------------------------------------------
    // 1. Get distinct symbols
    // ------------------------------------------------------------------
    info!("Fetching symbol list...");
    let symbols_csv = run_dolt_sql(
        &dolt_dir,
        "SELECT DISTINCT symbol FROM final_a_stock_eod_price ORDER BY symbol",
    )?;

    let symbols: Vec<String> = symbols_csv
        .lines()
        .skip(1) // skip header
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    let total = if limit > 0 {
        symbols.len().min(limit)
    } else {
        symbols.len()
    };
    info!("Found {} symbols, exporting {}...", symbols.len(), total);

    // ------------------------------------------------------------------
    // 2. Export stock_basic (small table, one file)
    // ------------------------------------------------------------------
    info!("Exporting stock_basic...");
    let basic_csv = run_dolt_sql(
        &dolt_dir,
        "SELECT symbol, '' AS name, \
         CASE WHEN exchange = 'SZSE' THEN 'SZ' WHEN exchange = 'SHSE' THEN 'SH' ELSE exchange END AS exchange, \
         list_date, delist_date FROM ts_a_stock_list",
    )?;

    let duck = Connection::open_in_memory()?;
    let basic_path = std::env::temp_dir().join("compass_basic.csv");
    std::fs::write(&basic_path, &basic_csv)?;
    let sql = format!(
        "COPY (SELECT * FROM read_csv('{}', header=true)) TO '{}/stock_basic.parquet' (FORMAT PARQUET)",
        basic_path.display(),
        output.display(),
    );
    duck.execute_batch(&sql)?;
    let _ = std::fs::remove_file(&basic_path);
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

    for symbol in symbols.iter().take(total) {
        let code = strip_prefix(symbol);
        pb.set_message(code.to_string());

        // Fetch CSV for this symbol
        let query = format!(
            "SELECT tradedate, open, high, low, close, adjclose, volume, amount \
             FROM final_a_stock_eod_price \
             WHERE symbol = '{symbol}' \
             ORDER BY tradedate"
        );
        let csv_data = match run_dolt_sql(&dolt_dir, &query) {
            Ok(data) => data,
            Err(e) => {
                warn!("dolt query failed for {symbol}: {e}");
                pb.inc(1);
                continue;
            }
        };

        if csv_data.lines().count() <= 1 {
            pb.inc(1);
            continue;
        }

        // Write CSV to temp, read into DuckDB, export to Parquet
        let csv_path = work_dir.join(format!("{code}.csv"));
        std::fs::write(&csv_path, &csv_data)?;

        let parquet_path = output.join("stock_daily").join(format!("{code}.parquet"));
        if let Some(parent) = parquet_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let sql = format!(
            "COPY (SELECT * FROM read_csv('{}', header=true)) TO '{}' (FORMAT PARQUET)",
            csv_path.display(),
            parquet_path.display(),
        );
        if let Err(e) = duck.execute_batch(&sql) {
            warn!("DuckDB export failed for {code}: {e}");
        }

        let _ = std::fs::remove_file(&csv_path);
        pb.inc(1);
    }

    // Remove temp work directory if empty
    let _ = std::fs::remove_dir(&work_dir);

    pb.finish_with_message("Done!");

    // ------------------------------------------------------------------
    // 4. Summary
    // ------------------------------------------------------------------
    let file_count = std::fs::read_dir(output.join("stock_daily"))
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
    fn run_dolt_sql_returns_error_for_nonexistent_dir() {
        let result = run_dolt_sql(
            std::path::Path::new("/nonexistent/dolt/dir"),
            "SELECT 1",
        );
        assert!(result.is_err());
    }
}
