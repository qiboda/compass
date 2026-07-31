#!/usr/bin/env python3
"""Compass data pipeline CLI — fetch + import into Dolt.

Usage:
    uv run python main.py fetch stock_basic [--resume]
    uv run python main.py fetch fin_indicators [--years 2024,2025] [--incremental]
    uv run python main.py fetch balance_sheet [--years 2024,2025] [--incremental]
    uv run python main.py fetch income [--years 2024,2025] [--incremental]
    uv run python main.py fetch cash_flow [--years 2024,2025] [--incremental]
    uv run python main.py import stock_basic
    uv run python main.py import fin_indicators
    uv run python main.py import balance_sheet
    uv run python main.py import income
    uv run python main.py import cash_flow
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
    """Import stock_basic.csv into Dolt (legacy logic, unchanged)."""
    from common import dolt_sql, dolt_sql_csv, dolt_table_import

    csv_path = COLLECTORS_DIR / "stock_basic.csv"
    print("[import stock_basic]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found.", file=sys.stderr)
        return

    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")
    dolt_table_import("_tmp_sb", csv_path, timeout=120)

    dolt_sql("DELETE FROM stock_basic")
    sql = """
        INSERT INTO stock_basic (symbol, ts_code, code, market, name, list_date,
            industry, lead_stock, region, data_ts, industry_alt, member_count, update_date)
        SELECT symbol, ts_code, f12, f13, f14, f26,
            f100, f101, f102, f124,
            f127, CAST(f134 AS SIGNED), f221
        FROM _tmp_sb
    """
    dolt_sql(sql)
    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")

    stdout = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic")
    lines = stdout.strip().split("\n")
    total = lines[-1] if len(lines) > 1 else "?"
    print(f"  Done: {total} rows", file=sys.stderr)


def _import_fin_indicators() -> None:
    """Import RPT_LICO_FN_CPD.csv into Dolt (legacy logic, unchanged)."""
    from common import dolt_sql, dolt_sql_csv, dolt_table_import

    csv_path = COLLECTORS_DIR / "RPT_LICO_FN_CPD.csv"
    print("[import fin_indicators]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found.", file=sys.stderr)
        return

    dolt_sql("DROP TABLE IF EXISTS _tmp_fin")
    dolt_table_import("_tmp_fin", csv_path)

    dolt_sql("DELETE FROM fin_indicators")
    sql = """
        INSERT INTO fin_indicators (
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
              IN (SELECT symbol FROM stock_basic)
    """
    dolt_sql(sql, timeout=600)
    dolt_sql("DROP TABLE IF EXISTS _tmp_fin")

    stdout = dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")
    lines = stdout.strip().split("\n")
    total = lines[-1] if len(lines) > 1 else "?"
    dolt_sql(
        "UPDATE data_updates SET last_updated=CURDATE(), "
        "row_count=(SELECT COUNT(*) FROM fin_indicators) WHERE table_name='fin_indicators'"
    )
    print(f"  Done: {total} rows", file=sys.stderr)


def sync_investment_data(restart: bool = False) -> None:
    """Sync investment_data: fetch from chenditc, push to skwy fork."""
    invest_dir = PROJECT_ROOT / "investment_data"

    if not (invest_dir / ".dolt").exists():
        print("[sync-investment] ERROR: investment_data not found", file=sys.stderr)
        return

    def dolt(*args: str) -> subprocess.CompletedProcess:
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


# ── Main CLI ────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description="Compass data pipeline")
    sub = parser.add_subparsers(dest="command")

    fetch = sub.add_parser("fetch", help="Fetch data from EastMoney")
    fetch.add_argument(
        "target",
        choices=["stock_basic", "fin_indicators", "balance_sheet", "income", "cash_flow"],
    )
    fetch.add_argument("--years", default="", help="Years to fetch (financial tables)")
    fetch.add_argument("--resume", action="store_true", help="Resume (stock_basic only)")
    fetch.add_argument("--max-pages", type=int, default=200, help="Max pages (stock_basic only)")

    imp = sub.add_parser("import", help="Import CSV into Dolt")
    imp.add_argument(
        "target",
        choices=["stock_basic", "fin_indicators", "balance_sheet", "income", "cash_flow"],
    )

    sub.add_parser("sync", help="Fetch all + import all")
    inv = sub.add_parser("sync-investment", help="Sync investment_data from upstream")
    inv.add_argument("--restart", action="store_true")

    args = parser.parse_args()

    if args.command == "fetch":
        years = _parse_years(args.years)

        if args.target == "stock_basic":
            import fetch_stock_basic
            sys.argv = ["fetch_stock_basic", "--max-pages", str(args.max_pages)]
            if args.resume:
                sys.argv.append("--resume")
            asyncio.run(fetch_stock_basic.main())

        elif args.target == "fin_indicators":
            import fetch_fin_indicators
            sys.argv = ["fetch_fin_indicators"]
            if args.years:
                sys.argv.extend(["--years", args.years])
            if args.incremental:
                sys.argv.append("--incremental")
            asyncio.run(fetch_fin_indicators.main())

        elif args.target == "balance_sheet":
            import fetch_balance_sheet
            asyncio.run(fetch_balance_sheet.run(years=years))

        elif args.target == "income":
            import fetch_income
            asyncio.run(fetch_income.run(years=years))

        elif args.target == "cash_flow":
            import fetch_cash_flow
            asyncio.run(fetch_cash_flow.run(years=years))

    elif args.command == "import":
        if args.target == "stock_basic":
            _import_stock_basic()
        elif args.target == "fin_indicators":
            _import_fin_indicators()
        elif args.target == "balance_sheet":
            import fetch_balance_sheet
            fetch_balance_sheet.import_to_dolt()
        elif args.target == "income":
            import fetch_income
            fetch_income.import_to_dolt()
        elif args.target == "cash_flow":
            import fetch_cash_flow
            fetch_cash_flow.import_to_dolt()

    elif args.command == "sync":
        from common import dolt_sql

        # 1. stock_basic
        print("[sync] Fetching stock_basic...", file=sys.stderr)
        import fetch_stock_basic
        sys.argv = ["fetch_stock_basic", "--max-pages", "200"]
        asyncio.run(fetch_stock_basic.main())
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

        # Update data_updates for all tables
        print("\n[sync] Updating data_updates...", file=sys.stderr)
        for tbl in [
            "stock_basic", "fin_indicators",
            "fin_balance_sheet", "fin_income", "fin_cash_flow",
        ]:
            dolt_sql(
                f"UPDATE data_updates SET last_updated=CURDATE(), "
                f"row_count=(SELECT COUNT(*) FROM {tbl}) WHERE table_name='{tbl}'"
            )
        print("[sync] Complete.", file=sys.stderr)

    elif args.command == "sync-investment":
        sync_investment_data(args.restart)

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
