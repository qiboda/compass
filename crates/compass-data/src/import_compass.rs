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
    /// Concept board members (full-overwrite import, DELETE+rewrite semantics).
    ConceptMember,
    /// Capital main flow (主力资金流), incremental merge on (symbol, trade_date).
    MainFlow,
    /// Dragon list (龙虎榜), incremental merge on (symbol, trade_date, seat_type).
    DragonList,
    /// Block trades (大宗交易), incremental merge on (symbol, trade_date, price).
    BlockTrade,
    /// Institution surveys (机构调研), incremental merge on (symbol, survey_date, org_name).
    InstitutionSurvey,
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
            "concept_member" => Ok(CompassTable::ConceptMember),
            "capital_main_flow" => Ok(CompassTable::MainFlow),
            "dragon_list" => Ok(CompassTable::DragonList),
            "block_trade" => Ok(CompassTable::BlockTrade),
            "institution_survey" => Ok(CompassTable::InstitutionSurvey),
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
        CompassTable::ConceptMember => import_concept_member(&dolt_dir, &output),
        CompassTable::MainFlow => import_append_table(
            AppendTableSpec {
                table_name: "capital_main_flow",
                date_col: "trade_date",
                partition_cols: "symbol, trade_date",
                prefer_new: true,
            },
            &dolt_dir,
            &output,
            overwrite,
            since,
        ),
        CompassTable::DragonList => import_append_table(
            AppendTableSpec {
                table_name: "dragon_list",
                date_col: "trade_date",
                partition_cols: "symbol, trade_date, seat_type",
                prefer_new: true,
            },
            &dolt_dir,
            &output,
            overwrite,
            since,
        ),
        CompassTable::BlockTrade => import_append_table(
            AppendTableSpec {
                table_name: "block_trade",
                date_col: "trade_date",
                partition_cols: "symbol, trade_date, price",
                prefer_new: true,
            },
            &dolt_dir,
            &output,
            overwrite,
            since,
        ),
        CompassTable::InstitutionSurvey => import_append_table(
            AppendTableSpec {
                table_name: "institution_survey",
                date_col: "survey_date",
                partition_cols: "symbol, survey_date, org_name",
                prefer_new: true,
            },
            &dolt_dir,
            &output,
            overwrite,
            since,
        ),
    }
}

fn import_stock_basic(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting stock_basic...");
    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT symbol, \
         name, \
         CAST(list_date AS DATE) AS list_date, \
         CAST(delist_date AS DATE) AS delist_date, \
         board, \
         full_name, \
         CAST(total_share AS DOUBLE) AS total_share, \
         industry, \
         region \
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
    import_append_table(
        AppendTableSpec {
            table_name,
            date_col: "report_date",
            partition_cols: "symbol, report_date",
            prefer_new: false,
        },
        dolt_dir,
        output,
        overwrite,
        since,
    )
}

/// Table definition for [`import_append_table`].
struct AppendTableSpec<'a> {
    /// Dolt table name (also the parquet file base name).
    table_name: &'a str,
    /// Column used for the `--since` filter.
    date_col: &'a str,
    /// Primary-key columns, used to dedupe old parquet rows against new Dolt
    /// rows on the same key (comma-separated, also the output sort order).
    partition_cols: &'a str,
    /// Which version wins when both sides hold the same key: SEPA capital
    /// tables are DELETE+rewritten by collectors each run, so the Dolt state
    /// is newer and must win (`true`); financial rows never change after
    /// publication, so old-wins behavior is preserved (`false`).
    prefer_new: bool,
}

/// Import an append-style table (financial statements, SEPA capital-flow
/// tables) with optional incremental merge.
///
/// When both sides hold the same key, `prefer_new` selects which version
/// wins, see [`AppendTableSpec::prefer_new`].
fn import_append_table<'a>(
    spec: AppendTableSpec<'a>,
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let AppendTableSpec {
        table_name,
        date_col,
        partition_cols,
        prefer_new,
    } = spec;
    let parquet_name = format!("{table_name}.parquet");
    let path = output.join(&parquet_name);

    let date_filter = match since {
        Some(s) if !s.is_empty() => format!(" WHERE {date_col} >= '{s}'"),
        _ => String::new(),
    };
    let query = format!("SELECT * FROM {table_name}{date_filter} ORDER BY {partition_cols}");

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

        let priority_order = if prefer_new {
            "priority DESC"
        } else {
            "priority"
        };
        let tmp_path = work_dir.join(format!("{table_name}.merged.parquet"));
        let duck = Connection::open_in_memory()?;
        let sql = format!(
            "COPY (SELECT * FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY {partition_cols} ORDER BY {priority_order}) AS rn \
             FROM (SELECT *, 1 AS priority FROM read_parquet('{}') \
             UNION ALL SELECT *, 2 FROM read_parquet('{}'))) WHERE rn = 1 ORDER BY {partition_cols}) \
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

/// Import concept board members as a full overwrite.
///
/// Unlike the append-style tables, `concept_member` is version-tracked
/// (collectors DELETE + rewrite the whole table each run), so an incremental
/// ROW_NUMBER merge would leave removed members lingering in the parquet and
/// stale concepts would keep scoring. Always write the full Dolt state,
/// equivalent to `--overwrite` regardless of the flag.
fn import_concept_member(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting concept_member...");
    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT * FROM concept_member ORDER BY concept_code, symbol",
    )?;
    let path = output.join("concept_member.parquet");
    std::fs::write(&path, &data)?;
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

    const CONCEPT_MEMBER_SCHEMA: &str = "\
        CREATE TABLE concept_member (\
        concept_code VARCHAR(20) NOT NULL, \
        symbol VARCHAR(20) NOT NULL, \
        concept_name VARCHAR(50), \
        update_date DATE, \
        PRIMARY KEY (concept_code, symbol))";

    const MAIN_FLOW_SCHEMA: &str = "\
        CREATE TABLE capital_main_flow (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, \
        super_large_net DOUBLE, large_net DOUBLE, \
        medium_net DOUBLE, small_net DOUBLE, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date))";

    const DRAGON_LIST_SCHEMA: &str = "\
        CREATE TABLE dragon_list (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        seat_type VARCHAR(10) NOT NULL, \
        buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE, \
        institution_flag TINYINT, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date, seat_type))";

    const BLOCK_TRADE_SCHEMA: &str = "\
        CREATE TABLE block_trade (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        price DOUBLE NOT NULL, \
        volume DOUBLE, amount DOUBLE, \
        buyer VARCHAR(100), seller VARCHAR(100), \
        premium_rate DOUBLE, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date, price))";

    const INSTITUTION_SURVEY_SCHEMA: &str = "\
        CREATE TABLE institution_survey (\
        symbol VARCHAR(20) NOT NULL, \
        survey_date DATE NOT NULL, \
        org_name VARCHAR(100) NOT NULL, \
        survey_type VARCHAR(20), \
        update_date DATE, \
        PRIMARY KEY (symbol, survey_date, org_name))";

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
            .arg("CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), \
                  industry VARCHAR(50), list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), \
                  full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))")
            .output().expect("create table");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO stock_basic (symbol, name, industry, list_date, delist_date, board, full_name, total_share, region) \
                  VALUES ('SH600519', '贵州茅台', '白酒Ⅱ', '2001-08-27', NULL, '主板', '贵州茅台酒股份有限公司', 12.56e8, '贵州')")
            .output()
            .expect("insert");

        import_stock_basic(tmp.path(), tmp.path()).expect("import");

        let parquet = tmp.path().join("stock_basic.parquet");
        assert!(parquet.exists());
        assert!(parquet.metadata().unwrap().len() > 500);

        // New columns present with expected values
        let duck = duckdb::Connection::open_in_memory().unwrap();
        let (symbol, list_date, board, full_name, total_share, industry, region): (
            String,
            String,
            String,
            String,
            f64,
            String,
            String,
        ) = duck
            .query_row(
                &format!(
                    "SELECT symbol, strftime(list_date, '%Y-%m-%d'), board, full_name, total_share, industry, region \
                     FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet.display()
                ),
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(symbol, "SH600519");
        assert_eq!(list_date, "2001-08-27");
        assert_eq!(board, "主板");
        assert_eq!(full_name, "贵州茅台酒股份有限公司");
        assert!((total_share - 12.56e8).abs() < 1.0);
        assert_eq!(industry, "白酒Ⅱ");
        assert_eq!(region, "贵州");

        let has_exchange: i64 = duck
            .prepare(&format!(
                "SELECT COUNT(*) FROM (DESCRIBE SELECT * FROM read_parquet('{}')) \
                 WHERE column_name = 'exchange'",
                parquet.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(has_exchange, 0, "exchange column must be dropped");

        // delist_date column exists and is NULL for this row
        let delist_date: Option<String> = duck
            .query_row(
                &format!(
                    "SELECT CAST(delist_date AS VARCHAR) FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(delist_date, None);
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
    fn compass_table_from_str_all_valid_variants() {
        assert!(matches!(
            "stock_basic".parse::<CompassTable>(),
            Ok(CompassTable::StockBasic)
        ));
        assert!(matches!(
            "fin_indicators".parse::<CompassTable>(),
            Ok(CompassTable::FinIndicators)
        ));
        assert!(matches!(
            "fin_balance_sheet".parse::<CompassTable>(),
            Ok(CompassTable::FinBalanceSheet)
        ));
        assert!(matches!(
            "fin_income".parse::<CompassTable>(),
            Ok(CompassTable::FinIncome)
        ));
        assert!(matches!(
            "fin_cash_flow".parse::<CompassTable>(),
            Ok(CompassTable::FinCashFlow)
        ));
        assert!(matches!(
            "concept_member".parse::<CompassTable>(),
            Ok(CompassTable::ConceptMember)
        ));
        assert!(matches!(
            "capital_main_flow".parse::<CompassTable>(),
            Ok(CompassTable::MainFlow)
        ));
        assert!(matches!(
            "dragon_list".parse::<CompassTable>(),
            Ok(CompassTable::DragonList)
        ));
        assert!(matches!(
            "block_trade".parse::<CompassTable>(),
            Ok(CompassTable::BlockTrade)
        ));
        assert!(matches!(
            "institution_survey".parse::<CompassTable>(),
            Ok(CompassTable::InstitutionSurvey)
        ));
    }

    #[test]
    fn compass_table_from_str_invalid_variant() {
        let result = "unknown_table".parse::<CompassTable>();
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "unknown table: unknown_table");
    }

    #[test]
    fn compass_table_from_str_empty_string() {
        let result = "".parse::<CompassTable>();
        assert!(result.is_err());
    }

    #[test]
    fn fin_indicators_skips_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Create fin_indicators with full schema but insert 0 rows.
        // When dolt's parquet output for empty result is <500 bytes, the
        // "empty or tiny data, skipping" path is triggered (lines 106-109).
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        // No INSERT — table is empty
        // The import_fin_indicators call should not panic regardless of whether
        // the parquet output falls below the 500-byte threshold.
        let result = import_fin_indicators(tmp.path(), tmp.path(), false, None);
        assert!(result.is_ok(), "import with empty table should succeed");
    }

    #[test]
    fn fin_indicators_since_merge_with_existing_parquet() {
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

        // Insert data with report_date 2024-12-31
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2024-12-31', 1.5e11, 7e10, 59.0, '贵州茅台')")
            .output().expect("insert 2024");

        // First import — creates initial parquet
        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("first import");
        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists(), "initial parquet should exist");

        // Insert more data with later report_date
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66, '贵州茅台')")
            .output().expect("insert 2025");

        // Incremental import with since — triggers merge path
        import_fin_indicators(tmp.path(), tmp.path(), false, Some("2025-01-01"))
            .expect("incremental import");

        // Read back merged parquet and verify both rows exist
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 2, "merged parquet should have both rows");
    }

    fn setup_financial_table(dolt_dir: &std::path::Path, table_name: &str) {
        let schema = format!(
            "CREATE TABLE {table_name} (\
             symbol VARCHAR(20) NOT NULL, \
             report_date DATE NOT NULL, \
             total_assets DOUBLE, \
             total_liabilities DOUBLE, \
             shareholder_equity DOUBLE, \
             revenue DOUBLE, \
             net_profit DOUBLE, \
             PRIMARY KEY (symbol, report_date))"
        );
        Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_dir)
            .arg("sql")
            .arg("-q")
            .arg(&schema)
            .output()
            .unwrap_or_else(|_| panic!("create {table_name}"));

        Command::new("dolt")
            .arg("--data-dir").arg(dolt_dir)
            .arg("sql").arg("-q")
            .arg(format!(
                "INSERT INTO {table_name} (symbol, report_date, total_assets, total_liabilities, shareholder_equity, revenue, net_profit) VALUES \
                 ('SH600519', '2024-12-31', 2.6e11, 4.8e10, 2.1e11, 1.5e11, 7e10)"
            ))
            .output().unwrap_or_else(|_| panic!("insert {table_name}"));
    }

    #[test]
    fn run_fin_balance_sheet_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_balance_sheet");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            None,
        )
        .expect("run fin_balance_sheet");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(
            parquet.metadata().unwrap().len() > 500,
            "parquet should have data"
        );
    }

    #[test]
    fn run_fin_income_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_income");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIncome,
            false,
            None,
        )
        .expect("run fin_income");

        let parquet = tmp.path().join("fin_income.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(parquet.metadata().unwrap().len() > 500);
    }

    #[test]
    fn run_fin_cash_flow_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_cash_flow");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinCashFlow,
            false,
            None,
        )
        .expect("run fin_cash_flow");

        let parquet = tmp.path().join("fin_cash_flow.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(parquet.metadata().unwrap().len() > 500);
    }

    #[test]
    fn financial_table_skip_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Create fin_balance_sheet with minimal schema, 0 rows
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE fin_balance_sheet (\
                symbol VARCHAR(20) NOT NULL, \
                report_date DATE NOT NULL, \
                PRIMARY KEY (symbol, report_date))",
            )
            .output()
            .expect("create table");

        import_financial_table("fin_balance_sheet", tmp.path(), tmp.path(), false, None)
            .expect("import_financial_table");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(
            !parquet.exists(),
            "empty table should be skipped due to tiny data"
        );
    }

    fn read_parquet_row_count(path: &std::path::Path) -> usize {
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        duck.query_row(
            &format!("SELECT COUNT(*) FROM read_parquet('{}')", path.display()),
            [],
            |row| row.get(0),
        )
        .expect("count")
    }

    #[test]
    fn financial_table_since_merge_preserves_old_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE fin_balance_sheet (\
                symbol VARCHAR(20) NOT NULL, \
                report_date DATE NOT NULL, \
                total_assets DOUBLE, \
                net_profit DOUBLE, \
                PRIMARY KEY (symbol, report_date))",
            )
            .output()
            .expect("create table");

        // Insert 2024 data
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2024-12-31', 2.6e11, 7e10)")
            .output().expect("insert 2024");

        // First full import
        import_financial_table("fin_balance_sheet", tmp.path(), tmp.path(), false, None)
            .expect("first import");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(parquet.exists());
        assert_eq!(read_parquet_row_count(&parquet), 1);

        // Insert 2025 data
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2025-12-31', 2.8e11, 8e10)")
            .output().expect("insert 2025");

        // Incremental import with since — triggers merge path
        import_financial_table(
            "fin_balance_sheet",
            tmp.path(),
            tmp.path(),
            false,
            Some("2025-01-01"),
        )
        .expect("incremental import");

        // Should have both rows after merge
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "merge should preserve both rows"
        );
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

    #[test]
    fn concept_member_full_overwrite_propagates_deletion() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(CONCEPT_MEMBER_SCHEMA)
            .output()
            .expect("create table");

        // Version N: 50 members
        let mut values = String::new();
        for i in 0..50 {
            if i > 0 {
                values.push_str(", ");
            }
            values.push_str(&format!(
                "('C{:04}', 'SH{:06}', '概念{}', '2026-01-01')",
                i,
                600000 + i,
                i
            ));
        }
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(format!(
                "INSERT INTO concept_member (concept_code, symbol, concept_name, update_date) \
                 VALUES {values}"
            ))
            .output()
            .expect("insert 50 members");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::ConceptMember,
            false,
            None,
        )
        .expect("first import");
        let parquet = tmp.path().join("concept_member.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 50);

        // Version N+1: collector rewrote the table with 45 members (5 removed)
        let removed: Vec<String> = (0..5).map(|i| format!("'SH{:06}'", 600000 + i)).collect();
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(format!(
                "DELETE FROM concept_member WHERE symbol IN ({})",
                removed.join(", ")
            ))
            .output()
            .expect("delete 5 members");

        // Second import without --overwrite/--since must still fully overwrite
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::ConceptMember,
            false,
            None,
        )
        .expect("second import");

        assert_eq!(
            read_parquet_row_count(&parquet),
            45,
            "deleted members must not linger in parquet"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let stale: usize = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}') WHERE symbol IN ({})",
                    parquet.display(),
                    removed.join(", ")
                ),
                [],
                |row| row.get(0),
            )
            .expect("stale count");
        assert_eq!(stale, 0, "removed members must not exist in parquet");
    }

    #[test]
    fn run_sepa_capital_tables_export_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create main flow");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow, main_net_inflow_rate) \
                  VALUES ('SH600519', '2026-01-05', 1.2e8, 3.5)")
            .output()
            .expect("insert main flow");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(DRAGON_LIST_SCHEMA)
            .output()
            .expect("create dragon list");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount, sell_amount, net_amount, institution_flag) \
                  VALUES ('SH600519', '2026-01-05', '机构专用', 1e8, 2e8, -1e8, 1)")
            .output()
            .expect("insert dragon list");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(BLOCK_TRADE_SCHEMA)
            .output()
            .expect("create block trade");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO block_trade (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) \
                  VALUES ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券', -0.02)")
            .output()
            .expect("insert block trade");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(INSTITUTION_SURVEY_SCHEMA)
            .output()
            .expect("create institution survey");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO institution_survey (symbol, survey_date, org_name, survey_type) \
                  VALUES ('SH600519', '2026-01-05', '华夏基金', '现场调研')",
            )
            .output()
            .expect("insert institution survey");

        for (table, parquet_name) in [
            (CompassTable::MainFlow, "capital_main_flow.parquet"),
            (CompassTable::DragonList, "dragon_list.parquet"),
            (CompassTable::BlockTrade, "block_trade.parquet"),
            (
                CompassTable::InstitutionSurvey,
                "institution_survey.parquet",
            ),
        ] {
            run(
                tmp.path().to_path_buf(),
                tmp.path().to_path_buf(),
                table,
                false,
                None,
            )
            .expect("run table");
            let parquet = tmp.path().join(parquet_name);
            assert!(parquet.exists(), "{parquet_name} should exist after import");
            assert_eq!(
                read_parquet_row_count(&parquet),
                1,
                "{parquet_name} row count"
            );
        }
    }

    #[test]
    fn capital_table_since_merge_new_value_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create table");

        // Two rows: SZ000001 @ 01-04, SH600519 @ 01-05 (inflow 1.0)
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
                ('SZ000001', '2026-01-04', 0.5e8), \
                ('SH600519', '2026-01-05', 1.0e8)",
            )
            .output()
            .expect("insert initial");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first import");
        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Dolt updated: same PK 01-05 row corrected (1.0 → 2.0), plus new 01-06 row
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE capital_main_flow SET main_net_inflow = 2.0e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05'",
            )
            .output()
            .expect("update 01-05");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
                ('SH600519', '2026-01-06', 3.0e8)",
            )
            .output()
            .expect("insert 01-06");

        // Incremental import with since — triggers merge path
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        // 3 rows: 01-04 preserved, 01-05 new value wins, 01-06 added
        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "merge should keep old rows, replace updated PK, add new rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let inflow: f64 = duck
            .query_row(
                &format!(
                    "SELECT main_net_inflow FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-05'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("inflow");
        assert!(
            (inflow - 2.0e8).abs() < 1.0,
            "new Dolt value must override old parquet value, got {inflow}"
        );
    }

    #[test]
    fn dragon_list_merge_preserves_multiple_seat_types() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(DRAGON_LIST_SCHEMA)
            .output()
            .expect("create table");

        // Same symbol+date, two seat types (three-column PK)
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount) VALUES \
                ('SH600519', '2026-01-05', '机构专用', 1e8), \
                ('SH600519', '2026-01-05', '营业部', 2e8)",
            )
            .output()
            .expect("insert initial");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            None,
        )
        .expect("first import");
        let parquet = tmp.path().join("dragon_list.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Both rows corrected in Dolt
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("UPDATE dragon_list SET buy_amount = 3e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '机构专用'")
            .output()
            .expect("update institutional");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE dragon_list SET buy_amount = 4e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '营业部'",
            )
            .output()
            .expect("update broker");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "merge must not collapse distinct seat_type rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (seat_type, expected) in [("机构专用", 3e8), ("营业部", 4e8)] {
            let buy: f64 = duck
                .query_row(
                    &format!(
                        "SELECT buy_amount FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '{}'",
                        parquet.display(),
                        seat_type
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("buy amount");
            assert!(
                (buy - expected).abs() < 1.0,
                "{seat_type} buy amount must be new value, got {buy}"
            );
        }
    }

    #[test]
    fn capital_table_skip_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Empty table — import must warn-skip without creating a parquet
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create table");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("import with empty table should succeed");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert!(
            !parquet.exists(),
            "empty table should be skipped due to tiny data"
        );
    }
}
