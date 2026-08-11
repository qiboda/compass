"""Adversarial tests for issue #235 — SQL-level TRIM of string columns on import.

Every collector INSERT SELECT currently passes text columns through raw
(no TRIM), so ASCII-space-padded EastMoney values (e.g. '机构专用 ',
' 上证50 ') land in Dolt verbatim. These tests are RED against current
production code and GREEN after each INSERT SELECT wraps its text columns
in ``TRIM(col) AS col``.

The U+3000 (full-width space) cases are characterization tests: SQL
``TRIM()`` only strips U+0020, so U+3000 must survive BOTH before and after
the fix — they lock the pure-TRIM blind spot against future silent
semantic changes (e.g. an over-eager ``REPLACE(col, '　', '')``). They must
NEVER assert "no whitespace", or GREEN becomes unreachable.

Dolt CSV-import semantics verified empirically (2026-08-11):
- leading/trailing U+0020 around content survive the CSV import verbatim;
- whitespace-only cells and empty cells are converted to NULL on import;
- TRIM(NULL) IS NULL, so the empty/NULL boundary is stable across the fix;
- TRIM() does NOT strip U+3000 (E3 80 80).
"""

import csv
import io
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


def _hex(s: str) -> str:
    """Upper-case UTF-8 hex of a string — unambiguous byte-level assertion
    (a trailing U+0020 is invisible in dolt's CSV text output, HEX is not)."""
    return s.encode("utf-8").hex().upper()


def _last(stdout: str) -> str:
    lines = stdout.strip().split("\n")
    return lines[-1] if lines else ""


# ── Shared Dolt fixture ────────────────────────────────────────────────

_SB_DDL = """\
CREATE TABLE stock_basic (
    symbol VARCHAR(20) PRIMARY KEY, ts_code VARCHAR(20), code VARCHAR(20),
    name VARCHAR(100), list_date DATE, delist_date DATE, board VARCHAR(50),
    full_name VARCHAR(200), total_share DOUBLE, industry VARCHAR(100),
    region VARCHAR(100), update_date DATE
)"""

_DATA_UPDATES_DDL = """\
CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY,
    last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)
"""

# Mirrors the REAL fin_indicators schema (main.py FIN_INDICATORS_DDL,
# not the looser test_main.py copy): text columns are VARCHAR/TEXT exactly
# as production, so qdate/date_label/dividend_plan/dividend_year TRIM
# assertions exercise production column types.
_FIN_INDICATORS_DDL = """\
CREATE TABLE fin_indicators (
    symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL,
    update_date DATE, notice_date DATE, data_type VARCHAR(20),
    qdate VARCHAR(8), eitime DATE, data_year INT, date_label VARCHAR(10),
    secucode VARCHAR(20), name VARCHAR(100), trade_market VARCHAR(20),
    trade_market_code VARCHAR(20), trade_market_zjg VARCHAR(10),
    security_type VARCHAR(10), security_type_code VARCHAR(20),
    industry VARCHAR(50), board_code VARCHAR(10), board_name VARCHAR(50),
    ori_board_code VARCHAR(10), org_code VARCHAR(20), is_new INT,
    basic_eps DECIMAL(10,4), deduct_basic_eps DECIMAL(10,4),
    revenue DECIMAL(20,2), net_profit DECIMAL(20,2), roe DECIMAL(10,4),
    bps DECIMAL(10,4), cash_flow_per_share DECIMAL(10,4),
    gross_margin DECIMAL(10,4), revenue_yoy DECIMAL(10,4),
    net_profit_yoy DECIMAL(10,4), operating_profit_yoy DECIMAL(10,4),
    net_profit_qoq DECIMAL(10,4), shares_growth DECIMAL(10,4),
    dividend_plan TEXT, dividend_year VARCHAR(10),
    PRIMARY KEY (symbol, report_date)
)"""


@pytest.fixture
def dolt_env(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Callable[[str], str]]:
    """Init temp Dolt with full 12-col stock_basic + data_updates.

    Unlike test_balance_sheet.py's symbol-only stock_basic, the FULL schema
    is needed by _import_stock_basic's 12-column INSERT SELECT. Seeded with
    SZ000001/SZ000002 so symbol-filtered INSERT SELECTs land rows.
    """
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
        capture_output=True, text=True,
    )
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.name", "CI"],
        capture_output=True, text=True,
    )
    init = subprocess.run(
        ["dolt", "--data-dir", str(tmp_path), "init"],
        capture_output=True, text=True,
    )
    assert init.returncode == 0, init.stderr

    def dolt_sql_csv(sql: str) -> str:
        return subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
            capture_output=True, text=True,
        ).stdout

    dolt_sql_csv(
        f"{_SB_DDL}; "
        "INSERT INTO stock_basic VALUES "
        "('SZ000001','000001.SZ','000001','平安银行','2024-01-01',NULL,"
        "'主板','平安银行股份有限公司',1000000000,'银行','深圳','2024-01-01'),"
        "('SZ000002','000002.SZ','000002','万科A','2024-01-01',NULL,"
        "'主板','万科企业股份有限公司',1000000000,'房地产','深圳','2024-01-01')"
    )
    dolt_sql_csv(_DATA_UPDATES_DDL)
    dolt_sql_csv(_FIN_INDICATORS_DDL)

    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
    return tmp_path, dolt_sql_csv


# ── stock_basic (_import_stock_basic + stock_basic_official.csv) ──────

_SB_HEADER = [
    "symbol", "ts_code", "code", "name", "list_date", "delist_date",
    "board", "full_name", "total_share", "industry", "region", "update_date",
]


def _sb_row(
    symbol: str,
    name: str = "",
    board: str = "",
    full_name: str = "",
    industry: str = "",
    region: str = "",
) -> list[str]:
    row = [""] * len(_SB_HEADER)
    row[_SB_HEADER.index("symbol")] = symbol
    row[_SB_HEADER.index("ts_code")] = symbol.removeprefix("SH") + ".SH"
    row[_SB_HEADER.index("code")] = symbol.removeprefix("SH")
    row[_SB_HEADER.index("name")] = name
    row[_SB_HEADER.index("list_date")] = "2024-01-01"
    row[_SB_HEADER.index("board")] = board
    row[_SB_HEADER.index("full_name")] = full_name
    row[_SB_HEADER.index("total_share")] = "1000000000"
    row[_SB_HEADER.index("industry")] = industry
    row[_SB_HEADER.index("region")] = region
    row[_SB_HEADER.index("update_date")] = "2024-01-01"
    return row


class TestStockBasicTrim:
    """_import_stock_basic must TRIM name/board/full_name/industry/region."""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_SB_HEADER)
            writer.writerows(rows)

    def test_ascii_spaces_are_trimmed(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: padded text columns land verbatim today; must land trimmed."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "stock_basic_official.csv",
            [
                _sb_row(
                    "SH600000",
                    name=" 贵州茅台 ",
                    board=" 上证50 ",
                    full_name="贵州茅台酒股份有限公司  ",
                    industry=" 酿酒行业 ",
                    region=" 贵州 ",
                ),
                _sb_row(
                    "SH600001",
                    name=" 平安银行 ",
                    board=" 主板 ",
                    full_name="平安银行股份有限公司",
                    industry="银行 ",
                    region=" 深圳",
                ),
            ],
        )

        main_mod._import_stock_basic()

        dirty = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM stock_basic WHERE "
            "name <> TRIM(name) OR board <> TRIM(board) OR "
            "full_name <> TRIM(full_name) OR industry <> TRIM(industry) OR "
            "region <> TRIM(region)"
        ))
        assert dirty == "0", (
            f"all 5 text columns must be trimmed of leading/trailing U+0020, "
            f"got {dirty} dirty row(s)"
        )

        # byte-exact values (HEX is immune to invisible trailing spaces in
        # dolt's CSV text output)
        row = _last(dolt_sql_csv(
            "SELECT HEX(name), HEX(board), HEX(full_name), HEX(industry), "
            "HEX(region) FROM stock_basic WHERE symbol='SH600000'"
        ))
        assert row == ",".join(
            _hex(v) for v in [
                "贵州茅台", "上证50", "贵州茅台酒股份有限公司",
                "酿酒行业", "贵州",
            ]
        ), f"SH600000 text columns must be trimmed, got {row!r}"

    def test_fullwidth_space_is_preserved(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """U+3000 characterization: TRIM() strips U+0020 only, so U+3000 in
        the source must survive verbatim both before and after the fix."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "stock_basic_official.csv",
            [_sb_row("SH600000", name="贵州茅台\u3000", board="上证50\u3000")],
        )

        main_mod._import_stock_basic()

        row = _last(dolt_sql_csv(
            "SELECT HEX(name), HEX(board) FROM stock_basic "
            "WHERE symbol='SH600000'"
        ))
        name_fw, board_fw = "贵州茅台\u3000", "上证50\u3000"
        assert row == f"{_hex(name_fw)},{_hex(board_fw)}", (
            "U+3000 must NOT be stripped by the TRIM fix — got {row!r}"
        )

    def test_empty_and_whitespace_only_become_null(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Boundary: empty cell and whitespace-only cell both import as NULL
        (dolt CSV import converts them; TRIM(NULL) IS NULL keeps it stable
        across the fix)."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "stock_basic_official.csv",
            [
                _sb_row("SH600000", name=""),
                _sb_row("SH600001", name="   "),
            ],
        )

        main_mod._import_stock_basic()

        null_rows = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM stock_basic WHERE name IS NULL"
        ))
        assert null_rows == "2", (
            f"empty and whitespace-only name cells must land as NULL, "
            f"got {null_rows}"
        )


# ── fin_indicators (_import_fin_indicators + RPT_LICO_FN_CPD.csv) ──────

_FIN_HEADER = [
    "SECUCODE", "SECURITY_CODE", "REPORTDATE", "UPDATE_DATE", "NOTICE_DATE",
    "DATATYPE", "QDATE", "EITIME", "DATAYEAR", "DATEMMDD",
    "SECURITY_NAME_ABBR", "TRADE_MARKET", "TRADE_MARKET_CODE", "TRADE_MARKET_ZJG",
    "SECURITY_TYPE", "SECURITY_TYPE_CODE", "PUBLISHNAME", "BOARD_CODE",
    "BOARD_NAME", "ORI_BOARD_CODE", "ORG_CODE", "ISNEW", "BASIC_EPS",
    "DEDUCT_BASIC_EPS", "TOTAL_OPERATE_INCOME", "PARENT_NETPROFIT",
    "WEIGHTAVG_ROE", "BPS", "MGJYXJJE", "XSMLL", "YSTZ", "SJLTZ", "YSHZ",
    "SJLHZ", "ZXGXL", "ASSIGNDSCRPT", "PAYYEAR",
]


def _fin_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    name: str = "",
    industry: str = "",
    board_name: str = "",
    trade_market: str = "",
    trade_market_zjg: str = "",
    security_type: str = "",
    data_type: str = "",
    qdate: str = "",
    date_label: str = "",
    dividend_plan: str = "",
    dividend_year: str = "",
) -> list[str]:
    row = [""] * len(_FIN_HEADER)
    row[_FIN_HEADER.index("SECUCODE")] = secucode
    row[_FIN_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_FIN_HEADER.index("REPORTDATE")] = report_date
    row[_FIN_HEADER.index("SECURITY_NAME_ABBR")] = name
    row[_FIN_HEADER.index("PUBLISHNAME")] = industry
    row[_FIN_HEADER.index("BOARD_NAME")] = board_name
    row[_FIN_HEADER.index("TRADE_MARKET")] = trade_market
    row[_FIN_HEADER.index("TRADE_MARKET_ZJG")] = trade_market_zjg
    row[_FIN_HEADER.index("SECURITY_TYPE")] = security_type
    row[_FIN_HEADER.index("DATATYPE")] = data_type
    row[_FIN_HEADER.index("QDATE")] = qdate
    row[_FIN_HEADER.index("DATEMMDD")] = date_label
    row[_FIN_HEADER.index("ASSIGNDSCRPT")] = dividend_plan
    row[_FIN_HEADER.index("PAYYEAR")] = dividend_year
    return row


class TestFinIndicatorsTrim:
    """_import_fin_indicators must TRIM name/industry/board_name
    (SECURITY_NAME_ABBR/PUBLISHNAME/BOARD_NAME)."""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_FIN_HEADER)
            writer.writerows(rows)

    def test_ascii_spaces_are_trimmed(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: padded name/industry/board_name land verbatim today."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "RPT_LICO_FN_CPD.csv",
            [
                _fin_row(
                    name=" 平安银行 ",
                    industry=" 银行 ",
                    board_name=" 主板 ",
                ),
                _fin_row(
                    secucode="000002.SZ",
                    report_date="2023-12-31",
                    name=" 万科A ",
                    industry=" 房地产 ",
                    board_name="主板 ",
                ),
            ],
        )

        main_mod._import_fin_indicators()

        dirty = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_indicators WHERE "
            "name <> TRIM(name) OR industry <> TRIM(industry) OR "
            "board_name <> TRIM(board_name)"
        ))
        assert dirty == "0", (
            "name/industry/board_name must be trimmed, "
            f"got {dirty} dirty row(s)"
        )

        row = _last(dolt_sql_csv(
            "SELECT HEX(name), HEX(industry), HEX(board_name) "
            "FROM fin_indicators WHERE symbol='SZ000001'"
        ))
        assert row == ",".join(_hex(v) for v in ["平安银行", "银行", "主板"]), (
            f"SZ000001 text columns must be trimmed, got {row!r}"
        )

    def test_fullwidth_space_is_preserved(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """U+3000 characterization: preserved verbatim by the pure-TRIM fix."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "RPT_LICO_FN_CPD.csv",
            [_fin_row(name="平安银行\u3000", industry="银行\u3000")],
        )

        main_mod._import_fin_indicators()

        row = _last(dolt_sql_csv(
            "SELECT HEX(name), HEX(industry) FROM fin_indicators "
            "WHERE symbol='SZ000001'"
        ))
        name_fw, industry_fw = "平安银行\u3000", "银行\u3000"
        assert row == f"{_hex(name_fw)},{_hex(industry_fw)}", (
            "U+3000 must NOT be stripped by the TRIM fix — got {row!r}"
        )

    def test_empty_name_becomes_null(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Boundary: empty and whitespace-only name cells land as NULL."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "RPT_LICO_FN_CPD.csv",
            [
                _fin_row(name="", industry="", board_name=""),
                _fin_row(report_date="2023-12-31", name="   "),
            ],
        )

        main_mod._import_fin_indicators()

        null_rows = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_indicators WHERE "
            "name IS NULL AND industry IS NULL AND board_name IS NULL"
        ))
        assert null_rows == "2", (
            f"empty/whitespace-only name/industry/board_name must land as "
            f"NULL, got {null_rows}"
        )

    def test_additional_text_columns_are_trimmed(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: the 8 secondary text columns (trade_market, trade_market_zjg,
        security_type, data_type, qdate, date_label, dividend_plan,
        dividend_year) must also be TRIM'd — plan todo 6 wraps every text
        column of the INSERT SELECT, not just name/industry/board_name."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        self._write_csv(
            tmp_path / "RPT_LICO_FN_CPD.csv",
            [
                _fin_row(
                    name="平安银行",
                    trade_market=" 上海 ",
                    trade_market_zjg=" 沪市 ",
                    security_type=" 一般企业 ",
                    data_type=" 年报 ",
                    qdate=" 2024Q4 ",
                    date_label=" 1231 ",
                    dividend_plan=" 10派2元 ",
                    dividend_year=" 2024 ",
                ),
            ],
        )

        main_mod._import_fin_indicators()

        dirty = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_indicators WHERE "
            "trade_market <> TRIM(trade_market) OR "
            "trade_market_zjg <> TRIM(trade_market_zjg) OR "
            "security_type <> TRIM(security_type) OR "
            "data_type <> TRIM(data_type) OR "
            "qdate <> TRIM(qdate) OR "
            "date_label <> TRIM(date_label) OR "
            "dividend_plan <> TRIM(dividend_plan) OR "
            "dividend_year <> TRIM(dividend_year)"
        ))
        assert dirty == "0", (
            "8 secondary text columns must be trimmed, "
            f"got {dirty} dirty row(s)"
        )

        row = _last(dolt_sql_csv(
            "SELECT HEX(trade_market), HEX(trade_market_zjg), "
            "HEX(security_type), HEX(data_type), HEX(qdate), HEX(date_label), "
            "HEX(dividend_plan), HEX(dividend_year) "
            "FROM fin_indicators WHERE symbol='SZ000001'"
        ))
        assert row == ",".join(_hex(v) for v in [
            "上海", "沪市", "一般企业", "年报", "2024Q4", "1231", "10派2元", "2024",
        ]), (
            f"SZ000001 secondary text columns must be trimmed, got {row!r}"
        )


# ── F10 tables (fin_balance_sheet / fin_cash_flow / fin_income) ────────
# CSV header = module COLS with REPORT_DATE inserted after ORG_TYPE
# (the temp-table DDL carries REPORT_DATE but COLS does not).

_F10_TEXT_COLS = [
    "SECURITY_NAME_ABBR", "ORG_TYPE", "REPORT_TYPE", "REPORT_DATE_NAME",
    "CURRENCY", "OPINION_TYPE",
]


def _f10_header(cols: str) -> list[str]:
    header = cols.split(", ")
    header.insert(header.index("ORG_TYPE") + 1, "REPORT_DATE")
    return header


def _f10_row(
    header: list[str],
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    **text: str,
) -> list[str]:
    row = [""] * len(header)
    row[header.index("SECUCODE")] = secucode
    row[header.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[header.index("REPORT_DATE")] = report_date
    for col, val in text.items():
        row[header.index(col)] = val
    return row


class _F10TrimBase:
    """Shared RED/GREEN/U+3000/NULL shape for the three F10 importers.

    Subclasses pin: fetch module, dolt table, text columns under test.
    """

    MODULE = ""  # e.g. "fetch_balance_sheet"
    TABLE = ""  # e.g. "fin_balance_sheet"
    TEXT_COLS: list[str] = []
    TRIM_EXPECTED: dict[str, str] = {}

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(self._header)
            writer.writerows(rows)

    @property
    def _header(self) -> list[str]:
        return _f10_header(__import__(self.MODULE, fromlist=["COLS"]).COLS)

    def test_ascii_spaces_are_trimmed(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: every declared text column must land trimmed, none verbatim."""
        import importlib

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "f10.csv"
        self._write_csv(csv_path, [_f10_row(self._header, **self.TRIM_EXPECTED)])

        importlib.import_module(self.MODULE).import_to_dolt(csv_path)

        dirty_where = " OR ".join(f"{c} <> TRIM({c})" for c in self.TEXT_COLS)
        dirty = _last(dolt_sql_csv(
            f"SELECT COUNT(*) FROM {self.TABLE} WHERE {dirty_where}"
        ))
        assert dirty == "0", (
            f"{self.TEXT_COLS} must all be trimmed of leading/trailing "
            f"U+0020, got {dirty} dirty row(s)"
        )

        hex_cols = ", ".join(f"HEX({c})" for c in self.TEXT_COLS)
        row = _last(dolt_sql_csv(
            f"SELECT {hex_cols} FROM {self.TABLE} WHERE symbol='SZ000001'"
        ))
        # TRIM_EXPECTED values are the PADDED inputs; the assertion target is
        # the trimmed form (TRIM() strips U+0020 from both ends).
        expected = ",".join(
            _hex(self.TRIM_EXPECTED[c].strip()) for c in self.TEXT_COLS
        )
        assert row == expected, (
            f"byte-exact trimmed values expected {expected!r}, got {row!r}"
        )

    def test_fullwidth_space_is_preserved(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """U+3000 characterization: preserved verbatim (TRIM strips U+0020 only)."""
        import importlib

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "f10.csv"
        self._write_csv(
            csv_path,
            [
                _f10_row(
                    self._header,
                    SECURITY_NAME_ABBR="平安银行\u3000",
                    OPINION_TYPE="标准无保留意见\u3000",
                ),
            ],
        )

        importlib.import_module(self.MODULE).import_to_dolt(csv_path)

        row = _last(dolt_sql_csv(
            f"SELECT HEX(SECURITY_NAME_ABBR), HEX(OPINION_TYPE) "
            f"FROM {self.TABLE} WHERE symbol='SZ000001'"
        ))
        name_fw, opinion_fw = "平安银行\u3000", "标准无保留意见\u3000"
        assert row == f"{_hex(name_fw)},{_hex(opinion_fw)}", (
            "U+3000 must NOT be stripped by the TRIM fix — got {row!r}"
        )

    def test_empty_and_whitespace_only_become_null(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Boundary: empty and whitespace-only cells land as NULL, stable
        across the fix (TRIM(NULL) IS NULL)."""
        import importlib

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "f10.csv"
        self._write_csv(
            csv_path,
            [
                _f10_row(self._header),
                _f10_row(self._header, report_date="2023-12-31",
                         SECURITY_NAME_ABBR="   "),
            ],
        )

        importlib.import_module(self.MODULE).import_to_dolt(csv_path)

        null_where = " AND ".join(f"{c} IS NULL" for c in self.TEXT_COLS)
        null_rows = _last(dolt_sql_csv(
            f"SELECT COUNT(*) FROM {self.TABLE} WHERE {null_where}"
        ))
        assert null_rows == "2", (
            f"empty/whitespace-only {self.TEXT_COLS} must land as NULL, "
            f"got {null_rows}"
        )


class TestBalanceSheetTrim(_F10TrimBase):
    MODULE = "fetch_balance_sheet"
    TABLE = "fin_balance_sheet"
    TEXT_COLS = _F10_TEXT_COLS + ["LISTING_STATE"]
    TRIM_EXPECTED = {
        "SECURITY_NAME_ABBR": " 平安银行 ",
        "ORG_TYPE": " 股份制商业银行 ",
        "REPORT_TYPE": " 年度报告 ",
        "REPORT_DATE_NAME": " 2024 Annual ",
        "CURRENCY": " CNY ",
        "OPINION_TYPE": " 标准无保留意见 ",
        "LISTING_STATE": " 上市 ",
    }


class TestCashFlowTrim(_F10TrimBase):
    MODULE = "fetch_cash_flow"
    TABLE = "fin_cash_flow"
    TEXT_COLS = list(_F10_TEXT_COLS)
    TRIM_EXPECTED = {
        "SECURITY_NAME_ABBR": " 平安银行 ",
        "ORG_TYPE": " 股份制商业银行 ",
        "REPORT_TYPE": " 年度报告 ",
        "REPORT_DATE_NAME": " 2024 Annual ",
        "CURRENCY": " CNY ",
        "OPINION_TYPE": " 标准无保留意见 ",
    }


class TestIncomeTrim(_F10TrimBase):
    MODULE = "fetch_income"
    TABLE = "fin_income"
    TEXT_COLS = list(_F10_TEXT_COLS)
    TRIM_EXPECTED = {
        "SECURITY_NAME_ABBR": " 平安银行 ",
        "ORG_TYPE": " 股份制商业银行 ",
        "REPORT_TYPE": " 年度报告 ",
        "REPORT_DATE_NAME": " 2024 Annual ",
        "CURRENCY": " CNY ",
        "OPINION_TYPE": " 标准无保留意见 ",
    }


# ── institution_survey (gk-keyed dedup + TRIM) ─────────────────────────

_SVY_HEADER = [
    "SECUCODE", "SECURITY_CODE", "RECEIVE_START_DATE", "RECEIVE_OBJECT",
    "RECEIVE_WAY_EXPLAIN",
]


def _svy_row(
    secucode: str = "000001.SZ",
    receive_start: str = "2025-08-28 00:00:00",
    receive_object: str = "机构专用",
    receive_way: str = "电话会议",
) -> list[str]:
    return [secucode, secucode.split(".")[0], receive_start, receive_object, receive_way]


class TestInstitutionSurveyTrim:
    """institution_survey groups on HEX(RECEIVE_OBJECT) — the gk key must
    TRIM in lockstep with the stored column, or '机构专用'/'机构专用 ' stay
    separate groups forever."""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_SVY_HEADER)
            writer.writerows(rows)

    def test_trailing_space_merges_same_group(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: '机构专用 ' and '机构专用' are the same investigating
        institution — they must merge into ONE trimmed row (gk key
        HEX(TRIM(RECEIVE_OBJECT)) groups them). Today they land as 2 rows."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "svy.csv"
        self._write_csv(
            csv_path,
            [
                _svy_row(receive_object="机构专用 ", receive_way="电话会议 "),
                _svy_row(receive_object="机构专用", receive_way="电话会议 "),
            ],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM institution_survey WHERE symbol='SZ000001'"
        ))
        assert count == "1", (
            "'机构专用' and '机构专用 ' (U+0020) must collapse into one "
            f"group, got {count} rows"
        )
        row = _last(dolt_sql_csv(
            "SELECT HEX(org_name), HEX(survey_type) FROM institution_survey"
        ))
        assert row == f"{_hex('机构专用')},{_hex('电话会议')}", (
            f"merged row must be byte-exact trimmed, got {row!r}"
        )

    def test_fullwidth_space_groups_stay_distinct(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """U+3000 characterization: TRIM() does not strip U+3000, so
        '机构专用\\u3000' and '机构专用' keep distinct gk keys and distinct
        rows — both before and after the fix."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "svy.csv"
        self._write_csv(
            csv_path,
            [
                _svy_row(receive_object="机构专用\u3000", receive_way="电话会议\u3000"),
                _svy_row(receive_object="机构专用", receive_way="电话会议"),
            ],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM institution_survey WHERE symbol='SZ000001'"
        ))
        assert count == "2", (
            "U+3000 variants must NOT be merged by a pure-TRIM fix, "
            f"got {count} rows"
        )
        org_hexes = sorted(
            row["org_hex"]
            for row in csv.DictReader(io.StringIO(dolt_sql_csv(
                "SELECT HEX(org_name) AS org_hex FROM institution_survey"
            )))
        )
        assert org_hexes == sorted(
            [_hex("机构专用\u3000"), _hex("机构专用")]
        ), f"both org_name variants must be preserved verbatim, got {org_hexes!r}"

    def test_empty_receive_object_lands_empty_string(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Boundary: empty/whitespace-only RECEIVE_OBJECT imports as NULL
        (dolt CSV import), and Dolt's MAX() over an all-NULL group yields ''
        — so the row lands with org_name='' (NOT NULL never violated, no
        whitespace). Stable across the fix: TRIM(NULL) IS NULL keeps the
        same '' result."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "svy.csv"
        self._write_csv(
            csv_path,
            [
                _svy_row(receive_object=""),
                _svy_row(receive_object="   "),
            ],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey"))
        assert count == "1", (
            "both NULL-org rows share one (symbol, date, HEX(NULL)) group — "
            f"must land as a single row, got {count}"
        )
        empty = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM institution_survey "
            "WHERE org_name = '' AND org_name IS NOT NULL"
        ))
        assert empty == "1", (
            "empty/whitespace-only RECEIVE_OBJECT must land as org_name='' "
            f"(Dolt MAX-over-NULL quirk), got {empty} row(s) matching"
        )


# ── block_trade (SELECT DISTINCT + TRIM) ───────────────────────────────

_BT_HEADER = [
    "SECUCODE", "SECURITY_CODE", "TRADE_DATE",
    "DEAL_PRICE", "DEAL_VOLUME", "DEAL_AMT",
    "BUYER_NAME", "SELLER_NAME", "PREMIUM_RATIO",
]


def _bt_row(
    buyer: str = "华泰证券",
    seller: str = "国泰君安",
) -> list[str]:
    return [
        "000001.SZ", "000001", "2024-12-31 00:00:00",
        "12.5", "240000", "3000000", buyer, seller, "0.06",
    ]


class TestBlockTradeTrim:
    """block_trade dedups via SELECT DISTINCT — TRIM(BUYER_NAME)/
    TRIM(SELLER_NAME) must run inside the DISTINCT, or '华泰证券' and
    '华泰证券 ' stay two rows with two PKs."""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_BT_HEADER)
            writer.writerows(rows)

    def test_trailing_space_buyer_merges_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: two rows identical except buyer '华泰证券' vs '华泰证券 '
        must collapse into one trimmed row (DISTINCT over TRIM(BUYER_NAME))."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(
            csv_path,
            [_bt_row(buyer="华泰证券"), _bt_row(buyer="华泰证券 ")],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade"))
        assert count == "1", (
            "rows differing only in buyer U+0020 padding must merge into "
            f"one, got {count} rows"
        )
        row = _last(dolt_sql_csv("SELECT HEX(buyer), HEX(seller) FROM block_trade"))
        assert row == f"{_hex('华泰证券')},{_hex('国泰君安')}", (
            f"merged row must be byte-exact trimmed, got {row!r}"
        )

    def test_trailing_space_seller_merges_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: same collapse for SELLER_NAME padding."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(
            csv_path,
            [_bt_row(seller="国泰君安"), _bt_row(seller="国泰君安 ")],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade"))
        assert count == "1", (
            f"rows differing only in seller U+0020 padding must merge into "
            f"one, got {count} rows"
        )
        row = _last(dolt_sql_csv("SELECT HEX(buyer), HEX(seller) FROM block_trade"))
        assert row == f"{_hex('华泰证券')},{_hex('国泰君安')}", (
            f"merged row must be byte-exact trimmed, got {row!r}"
        )

    def test_fullwidth_space_buyer_rows_stay_distinct(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """U+3000 characterization: DISTINCT over TRIM() keeps '华泰证券\\u3000'
        and '华泰证券' as separate rows, before and after the fix."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(
            csv_path,
            [_bt_row(buyer="华泰证券\u3000"), _bt_row(buyer="华泰证券")],
        )

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade"))
        assert count == "2", (
            "U+3000 buyer variants must NOT be merged by a pure-TRIM fix, "
            f"got {count} rows"
        )
        buyer_hexes = sorted(
            row["buyer_hex"]
            for row in csv.DictReader(io.StringIO(dolt_sql_csv(
                "SELECT HEX(buyer) AS buyer_hex FROM block_trade"
            )))
        )
        assert buyer_hexes == sorted(
            [_hex("华泰证券\u3000"), _hex("华泰证券")]
        ), f"both buyer variants must be preserved verbatim, got {buyer_hexes!r}"

    def test_empty_buyer_lands_single_empty_string(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Boundary: empty/whitespace-only BUYER_NAME imports as NULL (dolt
        CSV import); SELECT DISTINCT collapses the NULLs into one row, which
        lands with buyer=''. Stable across the fix: TRIM(NULL) IS NULL keeps
        the DISTINCT collapse and the '' value."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(csv_path, [_bt_row(buyer=""), _bt_row(buyer="   ")])

        import_to_dolt(csv_path)

        count = _last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade"))
        assert count == "1", (
            "NULL buyers must collapse via SELECT DISTINCT into one row — "
            f"got {count} rows"
        )
        empty = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM block_trade WHERE buyer = ''"
        ))
        assert empty == "1", (
            "empty/whitespace-only BUYER_NAME must land as buyer='' "
            f"(no whitespace, no NULL), got {empty} row(s) matching"
        )
