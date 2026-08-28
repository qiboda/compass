//! Rust port of collectors/fetch_fin_indicators.py plus
//! main.py::_import_fin_indicators (epic #310, B4).
//!
//! Fetch uses the shared `financial::run` machinery with the
//! `RPT_LICO_FN_CPD` report name and `REPORTDATE` filter; the Dolt import is
//! specialised because the Python import lives in main.py and maps a smaller
//! set of API columns to the `fin_indicators` table.

use std::path::{Path, PathBuf};

use crate::config::csv_dir;
use crate::dolt::import_replace_table;
use crate::error::Result;
use crate::financial::{FinancialConfig, run as run_financial};

pub const REPORT_NAME: &str = "RPT_LICO_FN_CPD";
pub const FILTER_COLUMN: &str = "REPORTDATE";
pub const DOLT_TABLE: &str = "fin_indicators";
pub const START_YEAR: i32 = 2020;
pub const INITIAL_UPDATE_ANCHOR: &str = "2020-01-01";

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS fin_indicators (
    symbol varchar(20) NOT NULL,
    report_date date NOT NULL,
    update_date date,
    notice_date date,
    data_type varchar(20),
    qdate varchar(8),
    eitime datetime,
    data_year int,
    date_label varchar(10),
    secucode varchar(20),
    name varchar(100),
    trade_market varchar(20),
    trade_market_code varchar(20),
    trade_market_zjg varchar(10),
    security_type varchar(10),
    security_type_code varchar(20),
    industry varchar(50),
    board_code varchar(10),
    board_name varchar(50),
    ori_board_code varchar(10),
    org_code varchar(20),
    is_new tinyint,
    basic_eps double,
    deduct_basic_eps double,
    revenue double,
    net_profit double,
    roe double,
    bps double,
    cash_flow_per_share double,
    gross_margin double,
    revenue_yoy double,
    net_profit_yoy double,
    operating_profit_yoy double,
    net_profit_qoq double,
    shares_growth double,
    dividend_plan text,
    dividend_year varchar(10),
    PRIMARY KEY (symbol, report_date)
)"#;

const TMP_DDL: &str = r#"CREATE TABLE _tmp_fin (
    SECUCODE VARCHAR(100),
    SECURITY_CODE VARCHAR(100),
    REPORTDATE VARCHAR(100),
    UPDATE_DATE VARCHAR(100),
    NOTICE_DATE VARCHAR(100),
    DATATYPE VARCHAR(100),
    QDATE VARCHAR(100),
    EITIME VARCHAR(100),
    DATAYEAR VARCHAR(100),
    DATEMMDD VARCHAR(100),
    SECURITY_NAME_ABBR VARCHAR(100),
    TRADE_MARKET VARCHAR(100),
    TRADE_MARKET_CODE VARCHAR(100),
    TRADE_MARKET_ZJG VARCHAR(100),
    SECURITY_TYPE VARCHAR(100),
    SECURITY_TYPE_CODE VARCHAR(100),
    PUBLISHNAME VARCHAR(100),
    BOARD_CODE VARCHAR(100),
    BOARD_NAME VARCHAR(100),
    ORI_BOARD_CODE VARCHAR(100),
    ORG_CODE VARCHAR(100),
    ISNEW VARCHAR(100),
    BASIC_EPS DOUBLE,
    DEDUCT_BASIC_EPS DOUBLE,
    TOTAL_OPERATE_INCOME DOUBLE,
    PARENT_NETPROFIT DOUBLE,
    WEIGHTAVG_ROE DOUBLE,
    BPS DOUBLE,
    MGJYXJJE DOUBLE,
    XSMLL DOUBLE,
    YSTZ DOUBLE,
    SJLTZ DOUBLE,
    YSHZ DOUBLE,
    SJLHZ DOUBLE,
    ZXGXL DOUBLE,
    ASSIGNDSCRPT TEXT,
    PAYYEAR VARCHAR(100)
)"#;

// The financial::run path uses this config only for the fetch side; the
// import below is custom and does not use `cols`/`tmp_ddl` from the config.
const CFG: FinancialConfig = FinancialConfig {
    report_name: REPORT_NAME,
    filter_column: FILTER_COLUMN,
    dolt_table: DOLT_TABLE,
    start_year: START_YEAR,
    initial_anchor: INITIAL_UPDATE_ANCHOR,
    cols: "",
    ddl: DDL,
    tmp_ddl: TMP_DDL,
    tmp_name: "_tmp_fin",
    trim_text_cols: &[],
    non_incremental_uses_last_report_date: false,
};

/// Fetch fin_indicators into a CSV (incremental or period-enumerated).
pub async fn run(
    years: Option<&[i32]>,
    periods: &str,
    page_size: usize,
    incremental: bool,
) -> Result<PathBuf> {
    run_financial(&CFG, years, periods, page_size, incremental).await
}

/// Import RPT_LICO_FN_CPD.csv into Dolt `fin_indicators` (upsert).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };

    let symbol = "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)";
    let insert_sql = format!(
        "INSERT INTO {table} (\
             symbol, report_date, update_date, notice_date,\
             data_type, qdate, eitime, data_year, date_label,\
             secucode, name, trade_market, trade_market_code, trade_market_zjg,\
             security_type, security_type_code, industry,\
             board_code, board_name, ori_board_code, org_code, is_new,\
             basic_eps, deduct_basic_eps, revenue, net_profit, roe, bps,\
             cash_flow_per_share, gross_margin,\
             revenue_yoy, net_profit_yoy, operating_profit_yoy, net_profit_qoq,\
             shares_growth, dividend_plan, dividend_year\
         ) \
         SELECT {symbol} AS _sym, REPORTDATE AS _rpt, UPDATE_DATE AS _upd, NOTICE_DATE AS _ntc,\
                TRIM(DATATYPE) AS _dt, TRIM(QDATE) AS _qd, EITIME AS _eit,\
                DATAYEAR AS _dyr, TRIM(DATEMMDD) AS _dlbl,\
                SECUCODE AS _sec, TRIM(SECURITY_NAME_ABBR) AS _nm,\
                TRIM(TRADE_MARKET) AS _tm, TRADE_MARKET_CODE AS _tmc,\
                TRIM(TRADE_MARKET_ZJG) AS _tmz,\
                TRIM(SECURITY_TYPE) AS _st, SECURITY_TYPE_CODE AS _stc,\
                TRIM(PUBLISHNAME) AS _ind,\
                BOARD_CODE AS _bc, TRIM(BOARD_NAME) AS _bnm, ORI_BOARD_CODE AS _obc,\
                ORG_CODE AS _org, ISNEW AS _new,\
                BASIC_EPS AS _eps, DEDUCT_BASIC_EPS AS _dept,\
                TOTAL_OPERATE_INCOME AS _rev, PARENT_NETPROFIT AS _npr,\
                WEIGHTAVG_ROE AS _roe, BPS AS _bps,\
                MGJYXJJE AS _cfps, XSMLL AS _gm,\
                YSTZ AS _ryoy, SJLTZ AS _npyoy, YSHZ AS _opyoy, SJLHZ AS _nqoq,\
                ZXGXL AS _sg, TRIM(ASSIGNDSCRPT) AS _dplan, TRIM(PAYYEAR) AS _pyr\
         FROM _tmp_fin \
         WHERE {symbol} IN (SELECT symbol FROM stock_basic) \
         ON DUPLICATE KEY UPDATE \
             update_date=_upd, notice_date=_ntc,\
             data_type=_dt, qdate=_qd, eitime=_eit, data_year=_dyr,\
             date_label=_dlbl,\
             secucode=_sec, name=_nm, trade_market=_tm,\
             trade_market_code=_tmc, trade_market_zjg=_tmz,\
             security_type=_st, security_type_code=_stc, industry=_ind,\
             board_code=_bc, board_name=_bnm, ori_board_code=_obc,\
             org_code=_org, is_new=_new,\
             basic_eps=_eps, deduct_basic_eps=_dept, revenue=_rev,\
             net_profit=_npr, roe=_roe, bps=_bps,\
             cash_flow_per_share=_cfps, gross_margin=_gm,\
             revenue_yoy=_ryoy, net_profit_yoy=_npyoy,\
             operating_profit_yoy=_opyoy, net_profit_qoq=_nqoq,\
             shares_growth=_sg, dividend_plan=_dplan, dividend_year=_pyr",
        table = DOLT_TABLE,
        symbol = symbol,
    );

    import_replace_table(
        &path,
        "_tmp_fin",
        DDL,
        &insert_sql,
        DOLT_TABLE,
        "EastMoney datacenter RPT_LICO_FN_CPD",
        "MAX(report_date)",
        Some(TMP_DDL),
        true,
    )
    .await
}
