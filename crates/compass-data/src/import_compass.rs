//! Import data from `compass_data` Dolt repository into Parquet.
//!
//! Follows the same `dolt sql -r parquet` → `fs::write` pattern as `import_dolt.rs`.
//! Supports stock_basic (one file) and fin_indicators (partitioned by symbol).

use std::path::{Path, PathBuf};

use tracing::info;

use crate::import_dolt::run_dolt_sql_parquet;

/// Tables in compass_data that can be imported.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompassTable {
    StockBasic,
    FinIndicators,
}

impl std::str::FromStr for CompassTable {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stock_basic" => Ok(CompassTable::StockBasic),
            "fin_indicators" => Ok(CompassTable::FinIndicators),
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
) -> Result<(), Box<dyn std::error::Error>> {
    match table {
        CompassTable::StockBasic => import_stock_basic(&dolt_dir, &output),
        CompassTable::FinIndicators => import_fin_indicators(&dolt_dir, &output, overwrite),
    }
}

/// Export stock_basic as a single Parquet file.
fn import_stock_basic(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting stock_basic...");

    let data = run_dolt_sql_parquet(dolt_dir, "SELECT * FROM stock_basic")?;
    let path = output.join("stock_basic.parquet");
    std::fs::write(&path, &data)?;
    info!("  → {}", path.display());
    Ok(())
}

/// Export fin_indicators as a single Parquet file (473K rows fits in one file).
fn import_fin_indicators(
    dolt_dir: &Path,
    output: &Path,
    _overwrite: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting fin_indicators...");

    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT report_date, update_date, notice_date, \
         data_type, qdate, data_year, date_label, \
         symbol, secucode, name, trade_market, trade_market_code, trade_market_zjg, \
         security_type, security_type_code, industry, \
         board_code, board_name, ori_board_code, org_code, is_new, \
         basic_eps, deduct_basic_eps, revenue, net_profit, roe, bps, \
         cash_flow_per_share, gross_margin, \
         revenue_yoy, net_profit_yoy, operating_profit_yoy, net_profit_qoq, \
         shares_growth, dividend_plan, dividend_year \
         FROM fin_indicators ORDER BY symbol, report_date",
    )?;

    let path = output.join("fin_indicators.parquet");
    std::fs::write(&path, &data)?;
    info!("  → {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
    fn import_stock_basic_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), industry VARCHAR(50))")
            .output().expect("create table");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO stock_basic VALUES ('SH600519', '贵州茅台', '白酒Ⅱ')")
            .output()
            .expect("insert");

        import_stock_basic(tmp.path(), tmp.path()).expect("import_stock_basic");

        let parquet = tmp.path().join("stock_basic.parquet");
        assert!(parquet.exists(), "parquet file not created");
        assert!(
            parquet.metadata().unwrap().len() > 500,
            "parquet file too small"
        );
    }

    #[test]
    fn import_fin_indicators_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg(
                "CREATE TABLE fin_indicators (\
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
                 PRIMARY KEY (symbol, report_date))")
            .output().expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66), \
                ('SZ000001', '2025-12-31', 1.42e11, 4.50e10, 2.35)")
            .output().expect("insert");

        import_fin_indicators(tmp.path(), tmp.path(), false).expect("import_fin_indicators");

        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists(), "parquet file not created");
        assert!(
            parquet.metadata().unwrap().len() > 500,
            "parquet file too small"
        );
    }
}
