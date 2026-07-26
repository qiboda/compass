#!/usr/bin/env python3
"""Compass data pipeline CLI — fetch + import into Dolt.

Usage:
    uv run python main.py fetch stock_basic [--resume]
    uv run python main.py fetch fin_indicators [--years 2024,2025] [--incremental]
    uv run python main.py import stock_basic
    uv run python main.py import fin_indicators
    uv run python main.py sync    # fetch all + import all
"""

import argparse
import asyncio
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
COLLECTORS_DIR = Path(__file__).resolve().parent
DOLT_DIR = PROJECT_ROOT / "compass_data"

CSV_STOCK = COLLECTORS_DIR / "stock_basic.csv"
CSV_FIN = COLLECTORS_DIR / "RPT_LICO_FN_CPD.csv"


def run_dolt(sql: str, **kwargs) -> subprocess.CompletedProcess:
    """Run a dolt SQL command against compass_data."""
    args = ["dolt", "--data-dir", str(DOLT_DIR), "sql"]
    if kwargs.get("csv"):
        args.extend(["-r", "csv"])
    args.extend(["-q", sql])
    return subprocess.run(args, capture_output=True, text=True, timeout=kwargs.get("timeout", 300))


def import_stock_basic():
    """Import stock_basic.csv into Dolt."""
    print("[import stock_basic]", file=sys.stderr)

    csv_path = CSV_STOCK
    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run 'fetch stock_basic' first.", file=sys.stderr)
        return

    run_dolt("DROP TABLE IF EXISTS _tmp_sb")
    result = subprocess.run(
        ["dolt", "--data-dir", str(DOLT_DIR), "table", "import", "-c", "_tmp_sb", "--continue", str(csv_path)],
        capture_output=True, text=True, timeout=120
    )
    if result.returncode != 0:
        print(f"  Import failed: {result.stderr}", file=sys.stderr)
        return

    run_dolt("DELETE FROM stock_basic")
    sql = """
        INSERT INTO stock_basic (symbol, ts_code, code, market, name, list_date,
            industry, lead_stock, region, data_ts, industry_alt, member_count, update_date)
        SELECT symbol, ts_code, f12, f13, f14, f26,
            f100, f101, f102, f124,
            f127, CAST(f134 AS SIGNED), f221
        FROM _tmp_sb
    """
    result = run_dolt(sql)
    if result.returncode != 0:
        print(f"  SQL error: {result.stderr}", file=sys.stderr)
    run_dolt("DROP TABLE IF EXISTS _tmp_sb")

    count = run_dolt("SELECT COUNT(*) FROM stock_basic", csv=True)
    lines = count.stdout.strip().split("\n")
    total = lines[-1] if len(lines) > 1 else "?"
    print(f"  Done: {total} rows", file=sys.stderr)


def import_fin_indicators():
    """Import RPT_LICO_FN_CPD.csv into Dolt."""
    print("[import fin_indicators]", file=sys.stderr)

    csv_path = CSV_FIN
    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run 'fetch fin_indicators' first.", file=sys.stderr)
        return

    run_dolt("DROP TABLE IF EXISTS _tmp_fin")
    result = subprocess.run(
        ["dolt", "--data-dir", str(DOLT_DIR), "table", "import", "-c", "_tmp_fin", "--continue", str(csv_path)],
        capture_output=True, text=True, timeout=300
    )
    if result.returncode != 0:
        print(f"  Import failed: {result.stderr}", file=sys.stderr)
        return

    run_dolt("DELETE FROM fin_indicators")
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
    """
    result = run_dolt(sql, timeout=600)
    if result.returncode != 0:
        print(f"  SQL error: {result.stderr}", file=sys.stderr)
    run_dolt("DROP TABLE IF EXISTS _tmp_fin")

    count = run_dolt("SELECT COUNT(*) FROM fin_indicators", csv=True)
    lines = count.stdout.strip().split("\n")
    total = lines[-1] if len(lines) > 1 else "?"
    run_dolt("UPDATE data_updates SET last_updated=CURDATE(), row_count=(SELECT COUNT(*) FROM fin_indicators) WHERE table_name='fin_indicators'")
    print(f"  Done: {total} rows", file=sys.stderr)


def sync_investment_data(restart: bool = False):
    """Sync investment_data: fetch from chenditc, push to skwy fork."""
    import os
    import signal

    invest_dir = PROJECT_ROOT / "investment_data"
    upstream = "origin"
    fork = "skwy"

    if not (invest_dir / ".dolt").exists():
        print("[sync-investment] ERROR: investment_data not found", file=sys.stderr)
        return

    dolt = lambda *args: subprocess.run(
        ["dolt", "--data-dir", str(invest_dir)] + list(args),
        capture_output=True, text=True, timeout=300
    )

    if restart:
        print("[sync-investment] Stopping Dolt SQL server...", file=sys.stderr)
        result = subprocess.run(["pkill", "-f", "dolt sql-server.*investment_data"], capture_output=True)
        if result.returncode == 0:
            print("  Server stopped", file=sys.stderr)

    print(f"[sync-investment] Fetching from {upstream}...", file=sys.stderr)
    dolt("fetch", upstream)

    print(f"[sync-investment] Merging {upstream}/master...", file=sys.stderr)
    dolt("checkout", "master")
    dolt("pull", upstream, "master")

    print(f"[sync-investment] Pushing to {fork}...", file=sys.stderr)
    result = dolt("push", fork, "master")
    if result.returncode == 0 or "up-to-date" in result.stderr + result.stdout:
        print("[sync-investment] Done.", file=sys.stderr)
    else:
        print(f"  Push issue: {result.stderr}", file=sys.stderr)

    if restart:
        server_script = PROJECT_ROOT / "scripts" / "start-dolt-server.sh"
        if server_script.exists():
            print("[sync-investment] Restarting server...", file=sys.stderr)
            subprocess.Popen(
                ["nohup", "bash", str(server_script)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True
            )


def main():
    parser = argparse.ArgumentParser(description="Compass data pipeline")
    sub = parser.add_subparsers(dest="command")

    fetch = sub.add_parser("fetch", help="Fetch data from EastMoney")
    fetch.add_argument("target", choices=["stock_basic", "fin_indicators"])
    fetch.add_argument("--years", default="", help="Years to fetch (fin_indicators only)")
    fetch.add_argument("--incremental", action="store_true", help="Incremental mode (fin_indicators only)")
    fetch.add_argument("--resume", action="store_true", help="Resume interrupted fetch (stock_basic only)")
    fetch.add_argument("--max-pages", type=int, default=200, help="Max pages (stock_basic only)")

    imp = sub.add_parser("import", help="Import CSV into Dolt")
    imp.add_argument("target", choices=["stock_basic", "fin_indicators"])

    sub.add_parser("sync", help="Fetch all + import all")

    inv = sub.add_parser("sync-investment", help="Sync investment_data from upstream (chenditc) to fork (skwy)")
    inv.add_argument("--restart", action="store_true", help="Stop server before sync, restart after")

    args = parser.parse_args()

    if args.command == "fetch":
        if args.target == "stock_basic":
            from fetch_stock_basic import main as run
            sys.argv = ["fetch_stock_basic", "--max-pages", str(args.max_pages)]
            if args.resume:
                sys.argv.append("--resume")
            asyncio.run(run())
        elif args.target == "fin_indicators":
            from fetch_fin_indicators import main as run
            sys.argv = ["fetch_fin_indicators"]
            if args.years:
                sys.argv.extend(["--years", args.years])
            if args.incremental:
                sys.argv.append("--incremental")
            asyncio.run(run())

    elif args.command == "import":
        if args.target == "stock_basic":
            import_stock_basic()
        elif args.target == "fin_indicators":
            import_fin_indicators()

    elif args.command == "sync":
        print("[sync] Fetching stock_basic...", file=sys.stderr)
        sys.argv = ["fetch_stock_basic", "--max-pages", "200"]
        from fetch_stock_basic import main as run_sb
        asyncio.run(run_sb())
        import_stock_basic()

        print("\n[sync] Fetching fin_indicators (incremental)...", file=sys.stderr)
        sys.argv = ["fetch_fin_indicators", "--incremental"]
        from fetch_fin_indicators import main as run_fi
        asyncio.run(run_fi())
        import_fin_indicators()

        print("\n[sync] Updating data_updates...", file=sys.stderr)
        run_dolt("UPDATE data_updates SET last_updated=CURDATE(), row_count=(SELECT COUNT(*) FROM stock_basic) WHERE table_name='stock_basic'")
        run_dolt("UPDATE data_updates SET last_updated=CURDATE(), row_count=(SELECT COUNT(*) FROM fin_indicators) WHERE table_name='fin_indicators'")
        print("[sync] Complete.", file=sys.stderr)

    elif args.command == "sync-investment":
        sync_investment_data(args.restart)

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
