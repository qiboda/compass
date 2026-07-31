//! Import data from `compass_data` Dolt repository into Parquet.
//!
//! Follows the same `dolt sql -r parquet` → `fs::write` pattern as `import_dolt.rs`.

use std::path::{Path, PathBuf};

use duckdb::Connection;
use tracing::{info, warn};

use crate::import_dolt::run_dolt_sql_parquet;

/// Tables in compass_data that can be imported.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompassTable {
    StockBasic,
    FinIndicators,
    FinBalanceSheet,
    FinIncome,
    FinCashFlow,
}

impl std::str::FromStr for CompassTable {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stock_basic" => Ok(CompassTable::StockBasic),
            "fin_indicators" => Ok(CompassTable::FinIndicators),
            "fin_balance_sheet" => Ok(CompassTable::FinBalanceSheet),
            "fin_income" => Ok(CompassTable::FinIncome),
            "fin_cash_flow" => Ok(CompassTable::FinCashFlow),
            _ => Err(format!("unknown table: {s}")),
        }
    }
}

/// Import data from compass_data Dolt into Parquet.
pub fn run(
    dolt_dir: PathBuf,
    output: PathBuf,
    table: CompassTable,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    match table {
        CompassTable::StockBasic => import_stock_basic(&dolt_dir, &output),
        CompassTable::FinIndicators => import_fin_indicators(&dolt_dir, &output, overwrite, since),
        CompassTable::FinBalanceSheet => {
            import_financial_table("fin_balance_sheet", &dolt_dir, &output, overwrite, since)
        }
        CompassTable::FinIncome => {
            import_financial_table("fin_income", &dolt_dir, &output, overwrite, since)
        }
        CompassTable::FinCashFlow => {
            import_financial_table("fin_cash_flow", &dolt_dir, &output, overwrite, since)
        }
    }
}

fn import_stock_basic(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting stock_basic...");
    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT RIGHT(symbol, 6) AS symbol, \
         name, \
         CASE LEFT(symbol, 2) WHEN 'SH' THEN 'SH' WHEN 'SZ' THEN 'SZ' WHEN 'BJ' THEN 'BJ' ELSE '' END AS exchange, \
         CAST(NULLIF(list_date, '-') AS DATE) AS list_date, \
         CAST(NULL AS DATE) AS delist_date \
         FROM stock_basic \
         WHERE symbol LIKE 'SH%' OR symbol LIKE 'SZ%' OR symbol LIKE 'BJ%' \
         ORDER BY symbol",
    )?;
    let path = output.join("stock_basic.parquet");
    std::fs::write(&path, &data)?;
    info!("  → {}", path.display());
    Ok(())
}

fn import_fin_indicators(
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = output.join("fin_indicators.parquet");

    let date_filter = match since {
        Some(s) if !s.is_empty() => format!(" WHERE report_date >= '{s}'"),
        _ => String::new(),
    };
    let query = format!(
        "SELECT report_date, update_date, notice_date, \
         data_type, qdate, data_year, date_label, \
         symbol, secucode, name, trade_market, trade_market_code, trade_market_zjg, \
         security_type, security_type_code, industry, \
         board_code, board_name, ori_board_code, org_code, is_new, \
         basic_eps, deduct_basic_eps, revenue, net_profit, roe, bps, \
         cash_flow_per_share, gross_margin, \
         revenue_yoy, net_profit_yoy, operating_profit_yoy, net_profit_qoq, \
         shares_growth, dividend_plan, dividend_year \
         FROM fin_indicators{} ORDER BY symbol, report_date",
        date_filter
    );

    info!("Exporting fin_indicators...");
    let new_data = run_dolt_sql_parquet(dolt_dir, &query)?;
    if new_data.len() < 500 {
        warn!("fin_indicators returned empty or tiny data, skipping");
        return Ok(());
    }

    if since.is_some() && !overwrite && path.exists() {
        // Incremental merge: old parquet (priority 1) + new dolt (priority 2)
        info!("Merging incremental data with existing parquet...");
        let work_dir = std::env::temp_dir().join("compass_parquet_work");
        std::fs::create_dir_all(&work_dir)?;

        let new_path = work_dir.join("fin.new.parquet");
        std::fs::write(&new_path, &new_data)?;

        let tmp_path = work_dir.join("fin.merged.parquet");
        let duck = Connection::open_in_memory()?;
        let sql = format!(
            "COPY (SELECT * FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY symbol, report_date ORDER BY priority) AS rn \
             FROM (SELECT *, 1 AS priority FROM read_parquet('{}') \
             UNION ALL SELECT *, 2 FROM read_parquet('{}'))) WHERE rn = 1 ORDER BY symbol, report_date) \
             TO '{}' (FORMAT PARQUET)",
            path.display(),
            new_path.display(),
            tmp_path.display(),
        );
        if let Err(e) = duck.execute_batch(&sql) {
            warn!("DuckDB merge failed: {e}, falling back to full export");
            std::fs::write(&path, &new_data)?;
        } else {
            std::fs::copy(&tmp_path, &path)?;
        }
        let _ = std::fs::remove_file(&new_path);
        let _ = std::fs::remove_file(&tmp_path);
    } else {
        std::fs::write(&path, &new_data)?;
    }

    info!("  → {}", path.display());
    Ok(())
}

fn import_financial_table(
    table_name: &str,
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parquet_name = format!("{table_name}.parquet");
    let path = output.join(&parquet_name);

    let date_filter = match since {
        Some(s) if !s.is_empty() => format!(" WHERE report_date >= '{s}'"),
        _ => String::new(),
    };
    let query = format!("SELECT * FROM {table_name}{date_filter} ORDER BY symbol, report_date");

    info!("Exporting {table_name}...");
    let new_data = run_dolt_sql_parquet(dolt_dir, &query)?;
    if new_data.len() < 500 {
        warn!("{table_name} returned empty or tiny data, skipping");
        return Ok(());
    }

    if since.is_some() && !overwrite && path.exists() {
        info!("Merging incremental data with existing parquet...");
        let work_dir = std::env::temp_dir().join("compass_parquet_work");
        std::fs::create_dir_all(&work_dir)?;

        let new_path = work_dir.join(format!("{table_name}.new.parquet"));
        std::fs::write(&new_path, &new_data)?;

        let tmp_path = work_dir.join(format!("{table_name}.merged.parquet"));
        let duck = Connection::open_in_memory()?;
        let sql = format!(
            "COPY (SELECT * FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY symbol, report_date ORDER BY priority) AS rn \
             FROM (SELECT *, 1 AS priority FROM read_parquet('{}') \
             UNION ALL SELECT *, 2 FROM read_parquet('{}'))) WHERE rn = 1 ORDER BY symbol, report_date) \
             TO '{}' (FORMAT PARQUET)",
            path.display(),
            new_path.display(),
            tmp_path.display(),
        );
        if let Err(e) = duck.execute_batch(&sql) {
            warn!("DuckDB merge failed: {e}, falling back to full export");
            std::fs::write(&path, &new_data)?;
        } else {
            std::fs::copy(&tmp_path, &path)?;
        }
        let _ = std::fs::remove_file(&new_path);
        let _ = std::fs::remove_file(&tmp_path);
    } else {
        std::fs::write(&path, &new_data)?;
    }

    info!("  → {}", path.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const FIN_SCHEMA: &str = "\
        CREATE TABLE fin_indicators (\
        symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
        update_date DATE, notice_date DATE, \
        data_type VARCHAR(20), qdate VARCHAR(8), data_year INT, date_label VARCHAR(10), \
        secucode VARCHAR(20), name VARCHAR(100), \
        trade_market VARCHAR(20), trade_market_code VARCHAR(20), trade_market_zjg VARCHAR(10), \
        security_type VARCHAR(10), security_type_code VARCHAR(20), industry VARCHAR(50), \
        board_code VARCHAR(10), board_name VARCHAR(50), ori_board_code INT, org_code VARCHAR(20), is_new TINYINT, \
        basic_eps DOUBLE, deduct_basic_eps DOUBLE, revenue DOUBLE, net_profit DOUBLE, roe DOUBLE, bps DOUBLE, \
        cash_flow_per_share DOUBLE, gross_margin DOUBLE, \
        revenue_yoy DOUBLE, net_profit_yoy DOUBLE, operating_profit_yoy DOUBLE, net_profit_qoq DOUBLE, \
        shares_growth DOUBLE, dividend_plan TEXT, dividend_year VARCHAR(10), \
        PRIMARY KEY (symbol, report_date))";

    fn setup_dolt(tmp: &std::path::Path) {
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
            .arg(tmp)
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    #[test]
    fn stock_basic_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), industry VARCHAR(50), list_date VARCHAR(20), member_count INT)")
            .output().expect("create table");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO stock_basic VALUES ('SH600519', '贵州茅台', '白酒Ⅱ', '2001-08-27', NULL)")
            .output()
            .expect("insert");

        import_stock_basic(tmp.path(), tmp.path()).expect("import");

        let parquet = tmp.path().join("stock_basic.parquet");
        assert!(parquet.exists());
        assert!(parquet.metadata().unwrap().len() > 500);
    }

    #[test]
    fn fin_indicators_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66)")
            .output().expect("insert");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("import");

        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists());
        assert!(parquet.metadata().unwrap().len() > 500);
    }

    #[test]
    fn parquet_data_matches_dolt_source() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg(
                "INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, roe, name) VALUES \
                 ('SH600519', '2025-12-31', 1720.54e8, 823.20e8, 65.66, 32.53, '贵州茅台'), \
                 ('SZ000001', '2025-12-31', 1000.00e8, 300.00e8, 2.50, 10.00, '平安银行')",
            )
            .output().expect("insert");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("import");

        let parquet_path = tmp.path().join("fin_indicators.parquet");
        assert!(parquet_path.exists());

        // Row count match
        let dolt_rows: usize = String::from_utf8(
            Command::new("dolt")
                .arg("--data-dir")
                .arg(tmp.path())
                .arg("sql")
                .arg("-r")
                .arg("csv")
                .arg("-q")
                .arg("SELECT COUNT(*) FROM fin_indicators")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

        let duck = duckdb::Connection::open_in_memory().unwrap();
        let parquet_rows: usize = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}')",
                    parquet_path.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dolt_rows, parquet_rows, "row count mismatch");

        // Data value match
        let revenue: f64 = duck
            .query_row(
                &format!(
                    "SELECT revenue FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet_path.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            (revenue - 1720.54e8).abs() < 1.0,
            "revenue mismatch: {revenue}"
        );

        // Symbol order preserved
        let symbols: Vec<String> = duck
            .prepare(&format!(
                "SELECT symbol FROM read_parquet('{}') ORDER BY symbol, report_date",
                parquet_path.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(symbols[0], "SH600519");
        assert_eq!(symbols[1], "SZ000001");
    }
}
