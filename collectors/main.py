#!/usr/bin/env python3
"""Compass data pipeline CLI — fetch + import into Dolt.

Usage:
    uv run python main.py fetch stock_basic
    uv run python main.py fetch fin_indicators [--years 2024,2025]
    uv run python main.py fetch balance_sheet [--years 2024,2025]
    uv run python main.py fetch income [--years 2024,2025]
    uv run python main.py fetch cash_flow [--years 2024,2025]
    uv run python main.py fetch dragon
    uv run python main.py fetch block_trade
    uv run python main.py fetch institution_survey
    uv run python main.py fetch concept_member
    uv run python main.py fetch main_flow
    uv run python main.py import stock_basic
    uv run python main.py import fin_indicators
    uv run python main.py import balance_sheet
    uv run python main.py import income
    uv run python main.py import cash_flow
    uv run python main.py import dragon
    uv run python main.py import block_trade
    uv run python main.py import institution_survey
    uv run python main.py import concept_member
    uv run python main.py import main_flow
    uv run python main.py sync              # fetch all + import all
    uv run python main.py sync-investment   # sync investment_data from upstream
"""

import argparse
import asyncio
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
COLLECTORS_DIR = Path(__file__).resolve().parent


def _parse_years(s: str) -> list[int] | None:
    if not s:
        return None
    return [int(y.strip()) for y in s.split(",") if y.strip()]


# ── Import helpers for existing (non-refactored) tables ─────────

def _import_stock_basic() -> None:
    """Import stock_basic_official.csv (SSE/SZSE/BSE official) into Dolt."""
    from common import dolt_sql, dolt_sql_csv, dolt_table_import

    csv_path = COLLECTORS_DIR / "stock_basic_official.csv"
    print("[import stock_basic]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found.", file=sys.stderr)
        return

    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")
    dolt_table_import("_tmp_sb", csv_path, timeout=120)

    dolt_sql("DELETE FROM stock_basic")
    # Column names match the Dolt schema directly; dolt table import already
    # typed the date/float columns and converted empty strings to NULL.
    sql = """
        INSERT INTO stock_basic (symbol, ts_code, code, name, list_date,
            delist_date, board, full_name, total_share, industry, region, update_date)
        SELECT symbol, ts_code, code, name, list_date,
            delist_date,
            board, full_name,
            total_share,
            industry, region, update_date
        FROM _tmp_sb
    """
    dolt_sql(sql)
    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")

    stdout = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic")
    lines = stdout.strip().split("\n")
    total = lines[-1] if len(lines) > 1 else "?"
    dolt_sql(
        "INSERT INTO data_updates (table_name, last_updated, source, row_count) "
        "VALUES ('stock_basic', CURDATE(), 'SSE/SZSE/BSE official', "
        f"{total if total != '?' else 0}) "
        "ON DUPLICATE KEY UPDATE last_updated=CURDATE(), source=VALUES(source), "
        "row_count=VALUES(row_count)"
    )
    print(f"  Done: {total} rows", file=sys.stderr)


FIN_INDICATORS_DDL = """
CREATE TABLE IF NOT EXISTS fin_indicators (
    symbol varchar(20) NOT NULL COMMENT '股票代码 (SZ000001)',
    report_date date NOT NULL COMMENT '报告期',
    update_date date COMMENT '数据最后更新日期',
    notice_date date COMMENT '公告日期',
    data_type varchar(20) COMMENT '报告类型 (2025年 年报)',
    qdate varchar(8) COMMENT '季度标签 (2025Q4)',
    eitime datetime COMMENT '精确发布时间',
    data_year int COMMENT '数据年份',
    date_label varchar(10) COMMENT '日期标签 (年报/一季报/...)',
    secucode varchar(20) COMMENT 'ts_code格式 (000001.SZ)',
    name varchar(100) COMMENT '证券简称',
    trade_market varchar(20) COMMENT '交易市场',
    trade_market_code varchar(20) COMMENT '交易市场代码',
    trade_market_zjg varchar(10) COMMENT '证监会市场代码',
    security_type varchar(10) COMMENT '证券类型',
    security_type_code varchar(20) COMMENT '证券类型代码',
    industry varchar(50) COMMENT '东财行业',
    board_code varchar(10) COMMENT '板块代码',
    board_name varchar(50) COMMENT '板块名称',
    ori_board_code varchar(10) COMMENT '原始板块代码',
    org_code varchar(20) COMMENT '机构代码',
    is_new tinyint COMMENT '是否新股',
    basic_eps double COMMENT '基本每股收益',
    deduct_basic_eps double COMMENT '扣非每股收益',
    revenue double COMMENT '营业总收入',
    net_profit double COMMENT '归母净利润',
    roe double COMMENT '加权净资产收益率(%)',
    bps double COMMENT '每股净资产',
    cash_flow_per_share double COMMENT '每股经营现金流',
    gross_margin double COMMENT '销售毛利率(%)',
    revenue_yoy double COMMENT '营收同比(%)',
    net_profit_yoy double COMMENT '净利同比(%)',
    operating_profit_yoy double COMMENT '营业利润同比(%)',
    net_profit_qoq double COMMENT '净利环比(%)',
    shares_growth double COMMENT '最新股本增长率',
    dividend_plan text COMMENT '分红方案',
    dividend_year varchar(10) COMMENT '分红年度',
    PRIMARY KEY (symbol, report_date)
)
"""


def _import_fin_indicators() -> int:
    """Import RPT_LICO_FN_CPD.csv into Dolt (merge semantics, ref #160).

    Rows are INSERT IGNORE'd into the existing fin_indicators table, deduped
    by the PK (symbol, report_date), so incremental-window CSVs append to
    history instead of clobbering it.
    """
    from common import import_replace_table

    print("[import fin_indicators]", file=sys.stderr)
    return import_replace_table(
        csv_path=COLLECTORS_DIR / "RPT_LICO_FN_CPD.csv",
        tmp_name="_tmp_fin",
        ddl=FIN_INDICATORS_DDL,
        insert_sql="""INSERT IGNORE INTO fin_indicators (
            symbol, report_date, update_date, notice_date,
            data_type, qdate, eitime, data_year, date_label,
            secucode, name, trade_market, trade_market_code, trade_market_zjg,
            security_type, security_type_code, industry,
            board_code, board_name, ori_board_code, org_code, is_new,
            basic_eps, deduct_basic_eps, revenue, net_profit, roe, bps,
            cash_flow_per_share, gross_margin,
            revenue_yoy, net_profit_yoy, operating_profit_yoy, net_profit_qoq,
            shares_growth, dividend_plan, dividend_year
        )
        SELECT
            CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
            REPORTDATE, UPDATE_DATE, NOTICE_DATE,
            DATATYPE, QDATE, EITIME, DATAYEAR, DATEMMDD,
            SECUCODE, SECURITY_NAME_ABBR, TRADE_MARKET, TRADE_MARKET_CODE, TRADE_MARKET_ZJG,
            SECURITY_TYPE, SECURITY_TYPE_CODE, PUBLISHNAME,
            BOARD_CODE, BOARD_NAME, ORI_BOARD_CODE, ORG_CODE, ISNEW,
            BASIC_EPS, DEDUCT_BASIC_EPS, TOTAL_OPERATE_INCOME, PARENT_NETPROFIT, WEIGHTAVG_ROE, BPS,
            MGJYXJJE, XSMLL,
            YSTZ, SJLTZ, YSHZ, SJLHZ,
            ZXGXL, ASSIGNDSCRPT, PAYYEAR
        FROM _tmp_fin
        WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
              IN (SELECT symbol FROM stock_basic)""",
        dolt_table="fin_indicators",
        source_label="EastMoney datacenter RPT_LICO_FN_CPD",
        last_report_expr="MAX(report_date)",
        merge=True,
    )


# ── sync_investment_data ────────────────────────────────────────

def sync_investment_data(restart: bool = False) -> None:
    """Sync investment_data: fetch from chenditc, push to skwy fork."""
    invest_dir = PROJECT_ROOT / "investment_data"

    if not (invest_dir / ".dolt").exists():
        print("[sync-investment] ERROR: investment_data not found", file=sys.stderr)
        return

    def dolt(*args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["dolt", "--data-dir", str(invest_dir)] + list(args),
            capture_output=True, text=True, timeout=300,
        )

    if restart:
        print("[sync-investment] Stopping Dolt SQL server...", file=sys.stderr)
        subprocess.run(["pkill", "-f", "dolt sql-server.*investment_data"], capture_output=True)

    print("[sync-investment] Fetching from origin...", file=sys.stderr)
    dolt("fetch", "origin")
    print("[sync-investment] Merging origin/master...", file=sys.stderr)
    dolt("checkout", "master")
    dolt("pull", "origin", "master")
    print("[sync-investment] Pushing to skwy...", file=sys.stderr)
    dolt("push", "skwy", "master")

    if restart:
        server_script = PROJECT_ROOT / "scripts" / "start-dolt-server.sh"
        if server_script.exists():
            print("[sync-investment] Restarting server...", file=sys.stderr)
            subprocess.Popen(
                ["nohup", "bash", str(server_script)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True,
            )

    print("[sync-investment] Done.", file=sys.stderr)


# ── Dispatch functions ──────────────────────────────────────────


def dispatch_fetch(
    target: str,
    years: list[int] | None = None,
) -> None:
    """Fetch data for the given target table.

    Args:
        target: One of stock_basic, fin_indicators, balance_sheet, income,
            cash_flow, main_flow, dragon, block_trade, institution_survey,
            concept_member.
        years: Years to fetch (financial tables only; defaults to sub-module default).
    """
    if target == "stock_basic":
        import fetch_stock_basic_official
        sys.argv = ["fetch_stock_basic_official"]
        fetch_stock_basic_official.main()

    elif target == "fin_indicators":
        import fetch_fin_indicators
        sys.argv = ["fetch_fin_indicators"]
        if years:
            sys.argv.extend(["--years", ",".join(str(y) for y in years)])
        asyncio.run(fetch_fin_indicators.main())

    elif target == "balance_sheet":
        import fetch_balance_sheet
        asyncio.run(fetch_balance_sheet.run(years=years))

    elif target == "income":
        import fetch_income
        asyncio.run(fetch_income.run(years=years))

    elif target == "cash_flow":
        import fetch_cash_flow
        asyncio.run(fetch_cash_flow.run(years=years))

    elif target == "dragon":
        import fetch_dragon
        asyncio.run(fetch_dragon.run())

    elif target == "block_trade":
        import fetch_block_trade
        asyncio.run(fetch_block_trade.run())

    elif target == "institution_survey":
        import fetch_institution_survey
        asyncio.run(fetch_institution_survey.run())

    elif target == "concept_member":
        import fetch_concept_member
        asyncio.run(fetch_concept_member.run())

    elif target == "main_flow":
        import fetch_main_flow
        asyncio.run(fetch_main_flow.run())


def dispatch_import(target: str) -> None:
    """Import CSV data into Dolt for the given target table.

    Args:
        target: One of stock_basic, fin_indicators, balance_sheet, income,
            cash_flow, main_flow, dragon, block_trade, institution_survey,
            concept_member.
    """
    if target == "stock_basic":
        _import_stock_basic()
    elif target == "fin_indicators":
        _import_fin_indicators()
    elif target == "balance_sheet":
        import fetch_balance_sheet
        fetch_balance_sheet.import_to_dolt()
    elif target == "income":
        import fetch_income
        fetch_income.import_to_dolt()
    elif target == "cash_flow":
        import fetch_cash_flow
        fetch_cash_flow.import_to_dolt()
    elif target == "dragon":
        import fetch_dragon
        fetch_dragon.import_to_dolt()
    elif target == "block_trade":
        import fetch_block_trade
        fetch_block_trade.import_to_dolt()
    elif target == "institution_survey":
        import fetch_institution_survey
        fetch_institution_survey.import_to_dolt()
    elif target == "concept_member":
        import fetch_concept_member
        fetch_concept_member.import_to_dolt()
    elif target == "main_flow":
        import fetch_main_flow
        fetch_main_flow.import_to_dolt()


def do_sync(restart: bool = False) -> None:
    """Fetch all tables from EastMoney, import into Dolt, and update data_updates.

    Args:
        restart: Reserved for future use; does not change sync behavior.
    """
    _ = restart  # reserved — no behavior change in sync subcommand

    from common import dolt_sql

    # 1. stock_basic
    print("[sync] Fetching stock_basic...", file=sys.stderr)
    import fetch_stock_basic_official
    sys.argv = ["fetch_stock_basic_official"]
    fetch_stock_basic_official.main()
    _import_stock_basic()

    # 2. fin_indicators
    print("\n[sync] Fetching fin_indicators (incremental)...", file=sys.stderr)
    import fetch_fin_indicators
    sys.argv = ["fetch_fin_indicators", "--incremental"]
    asyncio.run(fetch_fin_indicators.main())
    _import_fin_indicators()

    # 3. balance_sheet
    print("\n[sync] Fetching balance_sheet...", file=sys.stderr)
    import fetch_balance_sheet
    asyncio.run(fetch_balance_sheet.run())
    fetch_balance_sheet.import_to_dolt()

    # 4. income
    print("\n[sync] Fetching income...", file=sys.stderr)
    import fetch_income
    asyncio.run(fetch_income.run())
    fetch_income.import_to_dolt()

    # 5. cash_flow
    print("\n[sync] Fetching cash_flow...", file=sys.stderr)
    import fetch_cash_flow
    asyncio.run(fetch_cash_flow.run())
    fetch_cash_flow.import_to_dolt()

    # 6. dragon_list (龙虎榜席位)
    print("\n[sync] Fetching dragon_list...", file=sys.stderr)
    import fetch_dragon
    asyncio.run(fetch_dragon.run())
    fetch_dragon.import_to_dolt()

    # 7. block_trade (大宗交易)
    print("\n[sync] Fetching block_trade...", file=sys.stderr)
    import fetch_block_trade
    asyncio.run(fetch_block_trade.run())
    fetch_block_trade.import_to_dolt()

    # 8. institution_survey (机构调研)
    print("\n[sync] Fetching institution_survey...", file=sys.stderr)
    import fetch_institution_survey
    asyncio.run(fetch_institution_survey.run())
    fetch_institution_survey.import_to_dolt()

    # 9. concept_member (概念板块成分)
    print("\n[sync] Fetching concept_member...", file=sys.stderr)
    import fetch_concept_member
    asyncio.run(fetch_concept_member.run())
    fetch_concept_member.import_to_dolt()

    # 10. main_flow (主力资金流)
    print("\n[sync] Fetching main_flow...", file=sys.stderr)
    import fetch_main_flow
    asyncio.run(fetch_main_flow.run())
    fetch_main_flow.import_to_dolt()

    # Update data_updates for all tables
    print("\n[sync] Updating data_updates...", file=sys.stderr)
    for tbl in [
        "stock_basic", "fin_indicators",
        "fin_balance_sheet", "fin_income", "fin_cash_flow",
    ]:
        dolt_sql(
            f"INSERT INTO data_updates (table_name, last_updated, row_count) "
            f"VALUES ('{tbl}', CURDATE(), (SELECT COUNT(*) FROM {tbl})) "
            f"ON DUPLICATE KEY UPDATE last_updated=CURDATE(), "
            f"row_count=VALUES(row_count)"
        )
    print("[sync] Complete.", file=sys.stderr)


# ── Main CLI ────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description="Compass data pipeline")
    sub = parser.add_subparsers(dest="command")

    fetch = sub.add_parser("fetch", help="Fetch data from EastMoney")
    fetch.add_argument(
        "target",
        choices=["stock_basic", "fin_indicators", "balance_sheet", "income", "cash_flow", "dragon", "block_trade", "institution_survey", "concept_member", "main_flow"],
    )
    fetch.add_argument("--years", default="", help="Years to fetch (financial tables)")

    imp = sub.add_parser("import", help="Import CSV into Dolt")
    imp.add_argument(
        "target",
        choices=["stock_basic", "fin_indicators", "balance_sheet", "income", "cash_flow", "dragon", "block_trade", "institution_survey", "concept_member", "main_flow"],
    )

    sub.add_parser("sync", help="Fetch all + import all")
    inv = sub.add_parser("sync-investment", help="Sync investment_data from upstream")
    inv.add_argument("--restart", action="store_true")

    args = parser.parse_args()

    if args.command == "fetch":
        years = _parse_years(args.years)
        dispatch_fetch(args.target, years=years)

    elif args.command == "import":
        dispatch_import(args.target)

    elif args.command == "sync":
        do_sync()

    elif args.command == "sync-investment":
        sync_investment_data(args.restart)

    else:
        parser.print_help()


if __name__ == "__main__":  # pragma: no cover
    main()
