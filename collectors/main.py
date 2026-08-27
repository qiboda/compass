#!/usr/bin/env python3
"""Compass data pipeline CLI — fetch + import into Dolt.

Usage:
    uv run python main.py fetch stock_basic
    uv run python main.py fetch fin_indicators [--years 2024,2025]
    uv run python main.py fetch balance_sheet [--years 2024,2025] [--incremental]
    uv run python main.py fetch income [--years 2024,2025] [--incremental]
    uv run python main.py fetch cash_flow [--years 2024,2025] [--incremental]
    uv run python main.py fetch dragon
    uv run python main.py fetch block_trade
    uv run python main.py fetch institution_survey
    uv run python main.py fetch main_flow
    uv run python main.py import stock_basic
    uv run python main.py import fin_indicators
    uv run python main.py import balance_sheet
    uv run python main.py import income
    uv run python main.py import cash_flow
    uv run python main.py import dragon
    uv run python main.py import block_trade
    uv run python main.py import institution_survey
    uv run python main.py import main_flow
    uv run python main.py progress         # show live fetch progress
    uv run python main.py progress block_trade --json
    uv run python main.py sync              # fetch all + import all
    uv run python main.py sync-investment   # sync investment_data from upstream
"""

import argparse
import asyncio
import json
import os
import subprocess
import sys
from datetime import date, timedelta
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent


def _parse_years(s: str) -> list[int] | None:
    if not s:
        return None
    return [int(y.strip()) for y in s.split(",") if y.strip()]


# ── Import helpers for existing (non-refactored) tables ─────────


def _import_stock_basic() -> None:
    """Import stock_basic_official.csv (SSE/SZSE/BSE official) into Dolt.

    Epic #266 B1: when the name-en mapping is available the INSERT LEFT-JOINs
    it by ``TRIM(industry)`` to fill ``industry_en``; unmapped industries →
    NULL. The mapping staging table also gains suffix-stripped keys (Roman
    numerals like 白酒Ⅱ → 白酒) so both exact and base keys match; a missing
    mapping degrades gracefully — the base import always lands.
    """
    from common import (
        csv_dir,
        dolt_sql,
        dolt_sql_csv,
        dolt_table_import,
        drop_name_en_mapping,
        load_name_en_mapping,
    )

    csv_path = csv_dir() / "stock_basic_official.csv"
    print("[import stock_basic]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found.", file=sys.stderr)
        raise RuntimeError("stock_basic import: CSV not found; refusing to continue")

    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")
    if not dolt_table_import("_tmp_sb", csv_path, timeout=120):
        print("  ERROR: failed to stage stock_basic CSV into _tmp_sb", file=sys.stderr)
        raise RuntimeError("stock_basic import: dolt_table_import failed for _tmp_sb")

    tmp_lines = dolt_sql_csv("SELECT COUNT(*) FROM _tmp_sb").strip().split("\n")
    tmp_total = int(tmp_lines[-1]) if len(tmp_lines) > 1 else 0
    if tmp_total <= 0:
        dolt_sql("DROP TABLE IF EXISTS _tmp_sb")
        print("  ERROR: _tmp_sb is empty; refusing to overwrite stock_basic", file=sys.stderr)
        raise RuntimeError("stock_basic import: _tmp_sb is empty; refusing to clear stock_basic")

    before_lines = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic").strip().split("\n")
    before_total = int(before_lines[-1]) if len(before_lines) > 1 else 0
    if before_total > 0 and tmp_total < before_total // 2:
        dolt_sql("DROP TABLE IF EXISTS _tmp_sb")
        print(
            f"  ERROR: stock_basic candidate is too small ({tmp_total} < {before_total // 2}); "
            "refusing to clear the existing table",
            file=sys.stderr,
        )
        raise RuntimeError(
            "stock_basic import: candidate row count is too small; refusing to replace existing data"
        )

    def _checked_sql(sql: str, desc: str) -> None:
        result = dolt_sql(sql)
        if result.returncode != 0:
            raise RuntimeError(
                f"stock_basic import: {desc} failed: {getattr(result, 'stderr', '') or ''}".strip()
            )

    # Replace-by-rename: keep the previous table aside so any failure after the
    # rename can restore it. This avoids the old non-atomic DELETE-then-INSERT
    # that left no recovery if the INSERT or schema preparation failed.
    mapping = None
    failed = False
    renamed = False
    try:
        if before_total > 0:
            _checked_sql("DROP TABLE IF EXISTS _sb_backup", "drop old stock_basic backup")
            _checked_sql(
                "RENAME TABLE stock_basic TO _sb_backup",
                "rename stock_basic to backup",
            )
            renamed = True
            _checked_sql(
                "CREATE TABLE stock_basic LIKE _sb_backup",
                "recreate stock_basic schema",
            )

        mapping = load_name_en_mapping()
        # The fresh stock_basic is empty (recreated from backup or already
        # empty), so no DELETE is needed.
        if mapping:
            # Dual-key JOIN: exact TRIMmed industry, or its Roman-numeral
            # suffix stripped (白酒Ⅱ → 白酒) so suffixed industries hit the
            # base mapping key. The `<>` guard prevents double-match
            # inflation when the mapping holds both the suffixed and the
            # base key for one industry (review P2-1).
            join = """
                LEFT JOIN _tmp_name_en m
                  ON m.section = 'industry'
                 AND (m.`key` = TRIM(t.industry)
                      OR (TRIM(t.industry) REGEXP '[ⅠⅡⅢⅣⅤⅥⅦⅧⅨⅩ]$'
                          AND m.`key` <> TRIM(t.industry)
                          AND m.`key` = LEFT(TRIM(t.industry),
                                             CHAR_LENGTH(TRIM(t.industry)) - 1)))
            """
            insert_en_cols = ", industry_en"
            select_en_cols = ", m.value"
        else:
            join = ""
            insert_en_cols = ""
            select_en_cols = ""
        sql = f"""
            INSERT INTO stock_basic (symbol, ts_code, code, name, list_date,
                delist_date, board, full_name, total_share, industry, region,
                update_date{insert_en_cols})
            SELECT t.symbol, t.ts_code, t.code, TRIM(t.name), t.list_date,
                t.delist_date,
                TRIM(t.board), TRIM(t.full_name),
                t.total_share,
                TRIM(t.industry), TRIM(t.region), t.update_date{select_en_cols}
            FROM _tmp_sb t
            {join}
        """
        _checked_sql(sql, "insert stock_basic")
        # Validate the final table before dropping the backup, so a suspicious
        # empty result can still restore the previous stock_basic.
        stdout = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic")
        lines = stdout.strip().split("\n")
        total = lines[-1] if len(lines) > 1 else "?"
        if total in ("0", "?"):
            print("  ERROR: stock_basic final count is suspiciously empty", file=sys.stderr)
            raise RuntimeError("stock_basic import: final row count is empty")
    except Exception:
        failed = True
        if renamed:
            # Only restore when the original table has actually been renamed
            # aside. If the backup/rename preparation failed, the original
            # stock_basic is still intact and must not be touched.
            _checked_sql("DROP TABLE IF EXISTS stock_basic", "drop partial stock_basic")
            _checked_sql(
                "RENAME TABLE _sb_backup TO stock_basic",
                "restore stock_basic backup",
            )
        raise
    finally:
        if mapping is not None:
            drop_name_en_mapping()
        if before_total > 0 and not failed:
            _checked_sql("DROP TABLE IF EXISTS _sb_backup", "drop stock_basic backup")

    dolt_sql("DROP TABLE IF EXISTS _tmp_sb")

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


# Staging schema for RPT_LICO_FN_CPD.csv (CSV header columns, uppercase API
# names). Explicit DDL instead of Dolt `-c` inference: inference types
# numeric columns as FLOAT (32-bit), corrupting double precision on the
# INSERT SELECT round-trip; VARCHAR(100)/TEXT also avoids truncating raw
# (untrimmed) values at staging so the strict target insert fails loudly
# instead of silently truncating.
_TMP_FIN_DDL = """\
CREATE TABLE _tmp_fin (
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
)
"""


def _import_fin_indicators() -> int:
    """Import RPT_LICO_FN_CPD.csv into Dolt (UPSERT semantics, ref #135/#160).

    Rows are UPSERTed into the existing fin_indicators table keyed by the PK
    (symbol, report_date): new rows append to history while revised rows
    (same PK, moved UPDATE_DATE) OVERWRITE the old row across all 35 non-PK
    value columns. Writing constraint (Dolt 2.2.3): SELECT-side unique
    aliases + ODKU unqualified alias refs — qualified source refs
    (`_tmp_fin.COL`) fail on TRIM-wrapped text columns and VALUES() is
    unsupported. TRIM is done in the SELECT so the alias refs carry the
    trimmed value.

    The staging table uses an explicit DDL (create_sql) because Dolt's
    `table import -c` type inference types numeric columns as FLOAT (32-bit),
    which corrupts double precision on the INSERT SELECT round-trip.
    """
    from common import csv_dir, import_replace_table

    print("[import fin_indicators]", file=sys.stderr)
    return import_replace_table(
        csv_path=csv_dir() / "RPT_LICO_FN_CPD.csv",
        tmp_name="_tmp_fin",
        ddl=FIN_INDICATORS_DDL,
        create_sql=_TMP_FIN_DDL,
        insert_sql="""INSERT INTO fin_indicators (
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
            CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE) AS _sym,
            REPORTDATE AS _rpt,
            UPDATE_DATE AS _upd, NOTICE_DATE AS _ntc,
            TRIM(DATATYPE) AS _dt, TRIM(QDATE) AS _qd, EITIME AS _eit,
            DATAYEAR AS _dyr, TRIM(DATEMMDD) AS _dlbl,
            SECUCODE AS _sec, TRIM(SECURITY_NAME_ABBR) AS _nm,
            TRIM(TRADE_MARKET) AS _tm, TRADE_MARKET_CODE AS _tmc,
            TRIM(TRADE_MARKET_ZJG) AS _tmz,
            TRIM(SECURITY_TYPE) AS _st, SECURITY_TYPE_CODE AS _stc,
            TRIM(PUBLISHNAME) AS _ind,
            BOARD_CODE AS _bc, TRIM(BOARD_NAME) AS _bnm, ORI_BOARD_CODE AS _obc,
            ORG_CODE AS _org, ISNEW AS _new,
            BASIC_EPS AS _eps, DEDUCT_BASIC_EPS AS _dept,
            TOTAL_OPERATE_INCOME AS _rev, PARENT_NETPROFIT AS _npr,
            WEIGHTAVG_ROE AS _roe, BPS AS _bps,
            MGJYXJJE AS _cfps, XSMLL AS _gm,
            YSTZ AS _ryoy, SJLTZ AS _npyoy, YSHZ AS _opyoy, SJLHZ AS _nqoq,
            ZXGXL AS _sg, TRIM(ASSIGNDSCRPT) AS _dplan, TRIM(PAYYEAR) AS _pyr
        FROM _tmp_fin
        WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
              IN (SELECT symbol FROM stock_basic)
        ON DUPLICATE KEY UPDATE
            update_date=_upd, notice_date=_ntc,
            data_type=_dt, qdate=_qd, eitime=_eit, data_year=_dyr,
            date_label=_dlbl,
            secucode=_sec, name=_nm, trade_market=_tm,
            trade_market_code=_tmc, trade_market_zjg=_tmz,
            security_type=_st, security_type_code=_stc, industry=_ind,
            board_code=_bc, board_name=_bnm, ori_board_code=_obc,
            org_code=_org, is_new=_new,
            basic_eps=_eps, deduct_basic_eps=_dept, revenue=_rev,
            net_profit=_npr, roe=_roe, bps=_bps,
            cash_flow_per_share=_cfps, gross_margin=_gm,
            revenue_yoy=_ryoy, net_profit_yoy=_npyoy,
            operating_profit_yoy=_opyoy, net_profit_qoq=_nqoq,
            shares_growth=_sg, dividend_plan=_dplan, dividend_year=_pyr""",
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
            capture_output=True,
            text=True,
            timeout=300,
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
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )

    print("[sync-investment] Done.", file=sys.stderr)


# ── Dispatch functions ──────────────────────────────────────────


def dispatch_fetch(
    target: str,
    years: list[int] | None = None,
    incremental: bool = False,
) -> None:
    """Fetch data for the given target table.

    Args:
        target: One of stock_basic, fin_indicators, balance_sheet, income,
            cash_flow, main_flow, dragon, block_trade, institution_survey,
            index_daily.
        years: Years to fetch (financial tables only; defaults to sub-module default).
        incremental: For F10 financial tables, use UPDATE_DATE anchor fetch
            (issue #299). fin_indicators also supports this flag.
    """
    if target == "stock_basic":
        import fetch_stock_basic_official

        sys.argv = ["fetch_stock_basic_official"]
        fetch_stock_basic_official.main()

    elif target == "fin_indicators":
        import fetch_fin_indicators

        sys.argv = ["fetch_fin_indicators"]
        if incremental:
            sys.argv.append("--incremental")
        if years:
            sys.argv.extend(["--years", ",".join(str(y) for y in years)])
        asyncio.run(fetch_fin_indicators.main())

    elif target == "balance_sheet":
        import fetch_balance_sheet

        asyncio.run(fetch_balance_sheet.run(years=years, incremental=incremental))

    elif target == "income":
        import fetch_income

        asyncio.run(fetch_income.run(years=years, incremental=incremental))

    elif target == "cash_flow":
        import fetch_cash_flow

        asyncio.run(fetch_cash_flow.run(years=years, incremental=incremental))

    elif target == "dragon":
        import fetch_dragon

        asyncio.run(fetch_dragon.run())

    elif target == "block_trade":
        import fetch_block_trade

        asyncio.run(fetch_block_trade.run())

    elif target == "institution_survey":
        import fetch_institution_survey

        asyncio.run(fetch_institution_survey.run())

    elif target == "main_flow":
        import fetch_main_flow

        asyncio.run(fetch_main_flow.run())

    elif target == "index_daily":
        import fetch_index_daily

        asyncio.run(fetch_index_daily.run())


def _print_progress(data: dict[str, object]) -> None:
    """Print one progress record in a compact human-readable form."""
    name = data.get("name", "?")
    status = data.get("status", "?")
    percent = data.get("percent")
    percent_str = f"{percent:.1f}%" if isinstance(percent, (int, float)) else "n/a"
    print(f"[{name}] {status} {percent_str} — {data.get('message', '')}")
    total = data.get("total_items")
    if total is not None:
        print(
            f"  completed: {data.get('completed_items', 0)}/{total}  "
            f"rows: {data.get('fetched_rows', 0)}"
        )
    else:
        print(f"  rows: {data.get('fetched_rows', 0)}")
    if data.get("current_item"):
        print(f"  current: {data['current_item']}")
    if data.get("error"):
        print(f"  error: {data['error']}")


def dispatch_progress(target: str | None = None, as_json: bool = False) -> None:
    """Show live fetch progress from JSON files written by collectors.

    With ``target`` omitted, every ``*.progress.json`` in csv_dir() is shown.
    ``--json`` emits raw JSON for machine consumption.
    """
    from common import csv_dir, read_progress

    if target is not None:
        data = read_progress(target)
        if data is None:
            print(
                f"No progress file for {target} (fetch not started?)",
                file=sys.stderr,
            )
            raise SystemExit(1)
        if as_json:
            print(json.dumps(data, ensure_ascii=False, indent=2))
        else:
            _print_progress(data)
        return

    files = sorted(csv_dir().glob("*.progress.json"))
    if not files:
        if as_json:
            # Machine consumers expect valid JSON; emit an empty array
            # instead of an empty stdout that would fail json.loads("").
            print("[]")
        else:
            print("No fetch progress files found.", file=sys.stderr)
        return

    entries: list[dict[str, object]] = []
    for path in files:
        try:
            entries.append(json.loads(path.read_text(encoding="utf-8")))
        except (json.JSONDecodeError, OSError):
            continue

    if as_json:
        print(json.dumps(entries, ensure_ascii=False, indent=2))
    else:
        for data in entries:
            _print_progress(data)


def dispatch_import(target: str) -> None:
    """Import CSV data into Dolt for the given target table.

    Args:
        target: One of stock_basic, fin_indicators, balance_sheet, income,
            cash_flow, main_flow, dragon, block_trade, institution_survey,
            index_daily.
    """
    if target == "stock_basic":
        _import_stock_basic()
    elif target == "fin_indicators":
        _require_import(_import_fin_indicators(), "fin_indicators")
    elif target == "balance_sheet":
        import fetch_balance_sheet

        _require_import(fetch_balance_sheet.import_to_dolt(), "fin_balance_sheet")
    elif target == "income":
        import fetch_income

        _require_import(fetch_income.import_to_dolt(), "fin_income")
    elif target == "cash_flow":
        import fetch_cash_flow

        _require_import(fetch_cash_flow.import_to_dolt(), "fin_cash_flow")
    elif target == "dragon":
        import fetch_dragon

        _require_import(fetch_dragon.import_to_dolt(), "dragon_list")
    elif target == "block_trade":
        import fetch_block_trade

        _require_import(fetch_block_trade.import_to_dolt(), "block_trade")
    elif target == "institution_survey":
        import fetch_institution_survey

        _require_import(fetch_institution_survey.import_to_dolt(), "institution_survey")
    elif target == "main_flow":
        import fetch_main_flow

        _require_import(fetch_main_flow.import_to_dolt(), "capital_main_flow")

    elif target == "index_daily":
        import fetch_index_daily

        _require_import(fetch_index_daily.import_to_dolt(), "index_daily")


def _require_import(result: int, label: str) -> None:
    """Abort sync when an import reports zero rows.

    ``import_replace_table`` returns the full row count after import and only
    returns 0 when the CSV is missing or the SQL import failed, so a zero here
    must stop the pipeline instead of silently continuing.
    """
    if result == 0:
        raise RuntimeError(f"sync failed: {label} import returned 0 rows")


# ── issue #308 auto-heal wrappers (module-level so tests can monkeypatch) ──

DAILY_AUTO_HEAL_TABLES: list[tuple[str, str]] = [
    ("capital_main_flow", "trade_date"),
    ("index_daily", "trade_date"),
    ("dragon_list", "trade_date"),
    ("block_trade", "trade_date"),
]


def missing_dates(table: str, date_col: str, start: str, end: str) -> list[str]:
    """Thin wrapper over common.missing_dates (monkeypatchable by tests)."""
    from common import missing_dates as _missing_dates

    return _missing_dates(table, date_col, start, end)


def set_last_report_date(table: str, report_date: str) -> None:
    """Thin wrapper over common.set_last_report_date (monkeypatchable)."""
    from common import set_last_report_date as _set

    _set(table, report_date)


def _import_backfill_csv(path: Path, label: str, import_fn) -> None:
    """Import a backfill CSV when the source produced rows.

    Daily sources such as dragon/block_trade legitimately have zero records on
    some trading days; those fetchers remove their output file to signal
    "no data".  A missing file is a no-op, while an existing file must import
    with a positive row count (strict failure).
    """
    if not path.exists():
        print(
            f"[sync] Auto-heal: {label}: no rows in backfill range, skipping import",
            file=sys.stderr,
        )
        return
    _require_import(import_fn(path), label)


async def backfill(
    start_or_ranges: str | dict[str, tuple[str, str]],
    end: str | None = None,
) -> None:
    """Run per-source backfills for missing ranges.

    ``start_or_ranges`` may be a plain ``(start, end)`` pair (all four daily
    sources use the same range) or a dict mapping each daily table to its own
    ``(start, end)`` range.  The dict form avoids re-fetching already-present
    dates for sources with no or narrower gaps (issue #308 per-table healing).
    """
    import fetch_block_trade
    import fetch_dragon
    import fetch_index_daily
    import fetch_main_flow

    if isinstance(start_or_ranges, dict):
        ranges = start_or_ranges
    else:
        if end is None:
            raise ValueError("backfill: end date is required for a plain range")
        ranges = {table: (start_or_ranges, end) for table, _ in DAILY_AUTO_HEAL_TABLES}

    if "capital_main_flow" in ranges:
        start, end = ranges["capital_main_flow"]
        main_flow_path = await fetch_main_flow.backfill(start, end)
        _require_import(fetch_main_flow.import_to_dolt(main_flow_path), "capital_main_flow")

    if "index_daily" in ranges:
        start, end = ranges["index_daily"]
        index_path = await fetch_index_daily.backfill(start, end)
        _import_backfill_csv(index_path, "index_daily", fetch_index_daily.import_to_dolt)

    if "dragon_list" in ranges:
        start, end = ranges["dragon_list"]
        dragon_path = await fetch_dragon.run(start_date=start, end_date=end)
        _import_backfill_csv(dragon_path, "dragon_list", fetch_dragon.import_to_dolt)

    if "block_trade" in ranges:
        start, end = ranges["block_trade"]
        block_path = await fetch_block_trade.run(start=start, end=end)
        _import_backfill_csv(block_path, "block_trade", fetch_block_trade.import_to_dolt)


def _auto_heal_table_range(table: str, col: str) -> tuple[str, str]:
    """Return (start, end) for one daily table's gap scan.

    The end is today; the start is that table's own earliest existing trade
    date (issue #308: backfill from existing earliest, never pre-history),
    falling back to the last 90 days when the table has no rows yet.
    """
    from common import dolt_dir, dolt_sql_csv_strict

    # Exclude the current date: it is not a reliable "missing" until the next
    # day (market/EOD data may not be published yet in a morning run).
    end = (date.today() - timedelta(days=1)).isoformat()
    fallback_start = (date.today() - timedelta(days=90)).isoformat()
    if not (dolt_dir() / ".dolt").exists():
        return fallback_start, end

    # A fresh compass_data Dolt may not have the daily tables yet; they are
    # created later by the normal sync imports. Treat a missing table as empty
    # (90-day fallback) so first-run bootstrap is not blocked by auto-heal.
    exists_out = dolt_sql_csv_strict(
        f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table}'"
    )
    exists_lines = [line.strip() for line in exists_out.splitlines() if line.strip()]
    exists = len(exists_lines) > 1 and int(exists_lines[-1]) > 0
    if not exists:
        return fallback_start, end

    out = dolt_sql_csv_strict(f"SELECT MIN({col}) FROM {table}")
    lines = [line.strip() for line in out.splitlines() if line.strip()]
    value = lines[-1] if len(lines) > 1 else ""
    start = value if value and value != "NULL" else fallback_start
    return start, end


def do_sync(restart: bool = False) -> None:
    """Fetch all tables from EastMoney, import into Dolt, and update data_updates.

    Args:
        restart: Reserved for future use; does not change sync behavior.
    """
    _ = restart  # reserved — no behavior change in sync subcommand

    from common import dolt_sql

    # 0. Auto-heal missing daily rows (issue #308) — before any fetch/import.
    # Production always runs auto-heal strictly; tests disable it explicitly
    # through COMPASS_AUTO_HEAL=0 (conftest keeps legacy do_sync tests away
    # from real Dolt/network).
    if os.environ.get("COMPASS_AUTO_HEAL", "1") == "0":
        print("[sync] Auto-heal disabled (COMPASS_AUTO_HEAL=0)", file=sys.stderr)
    else:
        print("[sync] Auto-heal: checking missing trading dates...", file=sys.stderr)
        ranges: dict[str, tuple[str, str]] = {}
        total_missing = 0
        for table, col in DAILY_AUTO_HEAL_TABLES:
            table_start, table_end = _auto_heal_table_range(table, col)
            missing = missing_dates(table, col, table_start, table_end)
            if missing:
                print(
                    f"[sync] Auto-heal: {table} missing {len(missing)} dates",
                    file=sys.stderr,
                )
                ranges[table] = (min(missing), max(missing))
                total_missing += len(missing)
        if ranges:
            print(
                f"[sync] Auto-heal: backfilling {total_missing} dates per table",
                file=sys.stderr,
            )
            # Use an explicit event loop rather than asyncio.run() so the failure
            # of the (possibly mocked in unit tests) backfill coroutine always
            # propagates even when callers have patched asyncio.run itself.
            _loop = asyncio.new_event_loop()
            try:
                _loop.run_until_complete(backfill(ranges))
            finally:
                _loop.close()
            for table, (_start, end) in ranges.items():
                set_last_report_date(table, end)

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
    _require_import(_import_fin_indicators(), "fin_indicators")

    # 3. balance_sheet
    print("\n[sync] Fetching balance_sheet (incremental)...", file=sys.stderr)
    import fetch_balance_sheet

    asyncio.run(fetch_balance_sheet.run(incremental=True))
    _require_import(fetch_balance_sheet.import_to_dolt(), "fin_balance_sheet")

    # 4. income
    print("\n[sync] Fetching income (incremental)...", file=sys.stderr)
    import fetch_income

    asyncio.run(fetch_income.run(incremental=True))
    _require_import(fetch_income.import_to_dolt(), "fin_income")

    # 5. cash_flow
    print("\n[sync] Fetching cash_flow (incremental)...", file=sys.stderr)
    import fetch_cash_flow

    asyncio.run(fetch_cash_flow.run(incremental=True))
    _require_import(fetch_cash_flow.import_to_dolt(), "fin_cash_flow")

    # 6. dragon_list (龙虎榜席位)
    print("\n[sync] Fetching dragon_list...", file=sys.stderr)
    import fetch_dragon

    asyncio.run(fetch_dragon.run())
    _require_import(fetch_dragon.import_to_dolt(), "dragon_list")

    # 7. block_trade (大宗交易)
    print("\n[sync] Fetching block_trade...", file=sys.stderr)
    import fetch_block_trade

    asyncio.run(fetch_block_trade.run())
    _require_import(fetch_block_trade.import_to_dolt(), "block_trade")

    # 8. institution_survey (机构调研)
    print("\n[sync] Fetching institution_survey...", file=sys.stderr)
    import fetch_institution_survey

    asyncio.run(fetch_institution_survey.run())
    _require_import(fetch_institution_survey.import_to_dolt(), "institution_survey")

    # 10. main_flow (主力资金流)
    print("\n[sync] Fetching main_flow...", file=sys.stderr)
    import fetch_main_flow

    asyncio.run(fetch_main_flow.run())
    _require_import(fetch_main_flow.import_to_dolt(), "capital_main_flow")

    # 11. index_daily (指数日线: 官方指数/行业板块)
    print("\n[sync] Fetching index_daily...", file=sys.stderr)
    import fetch_index_daily

    asyncio.run(fetch_index_daily.run())
    _require_import(fetch_index_daily.import_to_dolt(), "index_daily")

    # Update data_updates for all tables
    print("\n[sync] Updating data_updates...", file=sys.stderr)
    for tbl in [
        "stock_basic",
        "fin_indicators",
        "fin_balance_sheet",
        "fin_income",
        "fin_cash_flow",
        "index_daily",
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
        choices=[
            "stock_basic",
            "fin_indicators",
            "balance_sheet",
            "income",
            "cash_flow",
            "dragon",
            "block_trade",
            "institution_survey",
            "main_flow",
            "index_daily",
        ],
    )
    fetch.add_argument("--years", default="", help="Years to fetch (financial tables)")
    fetch.add_argument(
        "--incremental",
        action="store_true",
        help="Use UPDATE_DATE incremental fetch for fin_indicators / balance_sheet / income / cash_flow",
    )

    imp = sub.add_parser("import", help="Import CSV into Dolt")
    imp.add_argument(
        "target",
        choices=[
            "stock_basic",
            "fin_indicators",
            "balance_sheet",
            "income",
            "cash_flow",
            "dragon",
            "block_trade",
            "institution_survey",
            "main_flow",
            "index_daily",
        ],
    )

    prog = sub.add_parser("progress", help="Show fetch progress")
    prog.add_argument(
        "target",
        nargs="?",
        choices=[
            "main_flow",
            "block_trade",
            "index_daily",
            "institution_survey",
            "dragon",
        ],
        default=None,
        help="Show progress for one collector (default: all)",
    )
    prog.add_argument("--json", action="store_true", help="Output raw JSON")

    sub.add_parser("sync", help="Fetch all + import all")
    inv = sub.add_parser("sync-investment", help="Sync investment_data from upstream")
    inv.add_argument("--restart", action="store_true")

    args = parser.parse_args()

    if args.command == "fetch":
        years = _parse_years(args.years)
        dispatch_fetch(args.target, years=years, incremental=args.incremental)

    elif args.command == "import":
        dispatch_import(args.target)

    elif args.command == "progress":
        dispatch_progress(args.target, as_json=args.json)

    elif args.command == "sync":
        do_sync()

    elif args.command == "sync-investment":
        sync_investment_data(args.restart)

    else:
        parser.print_help()


if __name__ == "__main__":  # pragma: no cover
    main()
