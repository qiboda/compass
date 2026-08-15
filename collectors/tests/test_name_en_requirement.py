"""Requirement-acceptance tests for epic #266 / B1 (#268) — data-name-i18n.

Verifies the *functional contract* the approved plan's B1 declares (happy
path + basic error path only; adversarial/boundary cases are the other
agent's remit):

- Acceptance #2: after import, Dolt ``index_basic`` carries ``name_en`` —
  a mapped symbol (e.g. SH000001) is non-NULL with the correct value; an
  unmapped symbol → NULL.
- Acceptance #3: after ``_import_stock_basic``, Dolt ``stock_basic`` carries
  ``industry_en`` — a mapped industry (e.g. "白酒Ⅱ") is non-NULL with the
  correct value; unmapped → NULL.
- Acceptance #4: the mapping table has a three-section schema (index
  symbol→name_en / industry zh→industry_en / concept name→name_en); the
  concept section is present with the correct format (consumed by B3, this
  batch only needs the file format to be parseable).
- Task scenario 4: "白酒Ⅱ" matched after TRIM — padded " 白酒Ⅱ " must hit
  the mapping key "白酒Ⅱ".
- Task scenario 3: missing mapping file must not break the import body —
  base columns still land, en columns are NULL.

Interface contract the GREEN implementation must honor (production side):
- ``name_en_mapping.csv`` header ``section,key,value``; ``section`` ∈
  {index, industry, concept}. ``index`` key = exchange symbol (e.g.
  SH000001), ``industry`` key = Chinese industry name (e.g. 白酒Ⅱ),
  ``concept`` key = concept Chinese name.
- Path injection hook: ``COMPASS_NAME_EN_MAPPING`` env points at the mapping
  CSV (tests build their own temp fixture here — production must honour this
  hook so tests do not depend on the checked-in file existing). When the
  env/file is missing, import proceeds with every en column NULL.
- ``fetch_index_daily.BASIC_DDL`` gains ``name_en VARCHAR(100)`` and
  ``_import_index_basic`` LEFT-JOINs the mapping on ``_tmp_ixb.symbol``.
- ``main._import_stock_basic`` LEFT-JOINs the mapping on ``TRIM(industry)``
  to populate ``industry_en`` (stock_basic DDL gains ``industry_en``).

These tests are RED against current production code (no name_en/industry_en
anywhere in collectors, verified 2026-08) and must all pass after GREEN.
"""

import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# Env hook the production mapping loader must honour (tests inject their own
# temp mapping file, never the checked-in one).
_MAPPING_ENV = "COMPASS_NAME_EN_MAPPING"

# Three-section mapping schema (acceptance #4). ``value`` is the English
# translation; ``key`` is symbol (index) / Chinese industry (industry) /
# Chinese concept (concept).
_MAPPING_HEADER = ["section", "key", "value"]
_MAPPING_ROWS = [
    ["index", "SH000001", "SSE Composite"],
    ["index", "SH000300", "CSI 300"],
    ["industry", "白酒Ⅱ", "Liquor"],
    ["industry", "半导体", "Semiconductors"],
    ["concept", "白酒概念", "Alcoholic Concept"],
]


def _last(stdout: str) -> str:
    """Last line of dolt csv output (header row + data rows)."""
    lines = stdout.strip().split("\n")
    return lines[-1] if lines else ""


def _write_csv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    with open(path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        writer.writerows(rows)


@pytest.fixture
def mapping_path(tmp_path: Path) -> Path:
    """Build the temporary three-section mapping CSV; return its path.

    Tests set COMPASS_NAME_EN_MAPPING to this path so they never depend on
    the production ``collectors/name_en_mapping.csv`` existing.
    """
    path = tmp_path / "name_en_mapping.csv"
    _write_csv(path, _MAPPING_HEADER, _MAPPING_ROWS)
    return path


# Shared temp-Dolt env: data_updates + 13-col stock_basic (with industry_en,
# mirroring the post-GREEN production schema). index_basic is deliberately NOT
# pre-created — ``_import_index_basic`` creates it via BASIC_DDL, which after
# GREEN carries name_en. stock_basic must pre-exist because `_import_stock_basic`
# only DELETE+INSERTs into it (mirrors test_trim_imports.py pattern).
_SB_DDL = """\
CREATE TABLE stock_basic (
    symbol VARCHAR(20) PRIMARY KEY, ts_code VARCHAR(20), code VARCHAR(20),
    name VARCHAR(100), list_date DATE, delist_date DATE, board VARCHAR(50),
    full_name VARCHAR(200), total_share DOUBLE, industry VARCHAR(100),
    region VARCHAR(100), update_date DATE, industry_en VARCHAR(100)
)"""

_DATA_UPDATES_DDL = """\
CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY,
    last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)
"""


@pytest.fixture
def dolt_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, Callable[[str], str]]:
    """Init temp Dolt with data_updates + full 13-col stock_basic."""
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.email", "req@compass.local"],
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.name", "ReqTest"],
        capture_output=True,
        text=True,
    )
    init = subprocess.run(
        ["dolt", "--data-dir", str(tmp_path), "init"],
        capture_output=True,
        text=True,
    )
    assert init.returncode == 0, init.stderr

    def dolt_sql_csv(sql: str) -> str:
        return subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
            capture_output=True,
            text=True,
        ).stdout

    dolt_sql_csv(f"{_SB_DDL}; " + _DATA_UPDATES_DDL)
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
    return tmp_path, dolt_sql_csv


# ── Mapping schema contract (acceptance #4) ────────────────────────────


def test_mapping_fixture_has_three_sections(mapping_path: Path) -> None:
    """The mapping fixture must parse into the three plan-declared sections.

    This locks the mapping CSV format the GREEN parser must read (header
    section,key,value; sections index/industry/concept). It validates the
    fixture/contract independent of production — not a RED driver, but the
    format contract production must honour.
    """
    sections: dict[str, dict[str, str]] = {s: {} for s in ("index", "industry", "concept")}
    with open(mapping_path, newline="", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        assert reader.fieldnames == _MAPPING_HEADER, reader.fieldnames
        for row in reader:
            assert row["section"] in sections, f"unknown section {row['section']!r}"
            assert row["key"] and row["value"], f"blank key/value in {row!r}"
            sections[row["section"]][row["key"]] = row["value"]

    # index: exchange symbol → name_en (acceptance #2 target)
    assert sections["index"]["SH000001"] == "SSE Composite"
    # industry: Chinese industry → industry_en (acceptance #3 target)
    assert sections["industry"]["白酒Ⅱ"] == "Liquor"
    # concept: in format for B3 (this batch only needs correct format)
    assert sections["concept"]["白酒概念"] == "Alcoholic Concept"


# ── index_basic.name_en (acceptance #2 + scenario 2) ───────────────────

_INDEX_BASIC_HEADER = ["symbol", "name", "index_type"]


class TestIndexBasicNameEn:
    def test_mapped_and_unmapped_symbols(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        mapping_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: mapped SH000001 → 'SSE Composite'; unmapped SH000999 → NULL."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setenv(_MAPPING_ENV, str(mapping_path))
        csv_path = tmp_path / "index_basic.csv"
        _write_csv(
            csv_path,
            _INDEX_BASIC_HEADER,
            [
                ["SH000001", "上证指数", "official"],
                ["SH000300", "沪深300", "official"],
                ["SH000999", "未知指数", "official"],
            ],
        )

        rows = import_to_dolt(csv_path)

        assert rows == 3
        # mapped symbol → correct English name (acceptance #2). COALESCE to a
        # non-empty marker so a NULL value is distinguishable from a missing
        # column (dolt renders a lone NULL row as an empty CSV line that the
        # _last() strip helper collapses into the header).
        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COALESCE(name_en, '<NULL>') FROM index_basic WHERE symbol='SH000001'"
                )
            )
            == "SSE Composite"
        )
        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COALESCE(name_en, '<NULL>') FROM index_basic WHERE symbol='SH000300'"
                )
            )
            == "CSI 300"
        )
        # unmapped symbol → NULL (scenario 2)
        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COUNT(*) FROM index_basic WHERE symbol='SH000999' AND name_en IS NULL"
                )
            )
            == "1"
        )

    def test_index_basic_ddl_has_name_en_column(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        mapping_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: BASIC_DDL must carry name_en (schema acceptance for the column)."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setenv(_MAPPING_ENV, str(mapping_path))
        csv_path = tmp_path / "index_basic.csv"
        _write_csv(
            csv_path,
            _INDEX_BASIC_HEADER,
            [
                ["SH000001", "上证指数", "official"],
            ],
        )

        import_to_dolt(csv_path)

        assert "name_en" in _last(dolt_sql_csv("SHOW COLUMNS FROM index_basic"))

    def test_import_without_mapping_degrades_gracefully(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        mapping_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED (scenario 3): missing mapping file must not break the import
        body — base columns land intact and en columns are NULL for all rows.

        Driven RED by the index path: with no feature the ``name_en`` column
        does not exist at all, so the ``name_en IS NULL`` count errors (returns
        an empty stdout → the assertion fails). The stock_basic half is a
        collateral assertion on the same degrade contract: without a mapping
        both importers must keep their base data and leave every en column
        NULL (vs. GREEN, which would crudely fill or crash).
        """
        import main as main_mod
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setenv(_MAPPING_ENV, str(tmp_path / "does_not_exist.csv"))
        # index_basic path
        csv_path = tmp_path / "index_basic.csv"
        _write_csv(
            csv_path,
            _INDEX_BASIC_HEADER,
            [
                ["SH000001", "上证指数", "official"],
            ],
        )
        rows = import_to_dolt(csv_path)
        assert rows == 1
        # base column intact despite missing mapping
        assert (
            _last(dolt_sql_csv("SELECT name FROM index_basic WHERE symbol='SH000001'"))
            == "上证指数"
        )
        # name_en NULL when no mapping was present
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM index_basic WHERE name_en IS NULL")) == "1"
        # stock_basic path — base industry intact, industry_en NULL
        _write_csv(
            tmp_path / "stock_basic_official.csv",
            _SB_HEADER,
            [
                _sb_row("SH600519", "贵州茅台", "白酒Ⅱ"),
            ],
        )
        main_mod._import_stock_basic()
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM stock_basic")) == "1"
        assert (
            _last(dolt_sql_csv("SELECT industry FROM stock_basic WHERE symbol='SH600519'"))
            == "白酒Ⅱ"
        )
        assert (
            _last(dolt_sql_csv("SELECT COUNT(*) FROM stock_basic WHERE industry_en IS NULL")) == "1"
        )


# ── stock_basic.industry_en (acceptance #3 + scenarios 2/4) ────────────

_SB_HEADER = [
    "symbol",
    "ts_code",
    "code",
    "name",
    "list_date",
    "delist_date",
    "board",
    "full_name",
    "total_share",
    "industry",
    "region",
    "update_date",
]


def _sb_row(
    symbol: str,
    name: str,
    industry: str,
) -> list[str]:
    row = [""] * len(_SB_HEADER)
    row[_SB_HEADER.index("symbol")] = symbol
    row[_SB_HEADER.index("ts_code")] = symbol.removeprefix("SH") + ".SH"
    row[_SB_HEADER.index("code")] = symbol.removeprefix("SH")
    row[_SB_HEADER.index("name")] = name
    row[_SB_HEADER.index("list_date")] = "2024-01-01"
    row[_SB_HEADER.index("board")] = "主板"
    row[_SB_HEADER.index("full_name")] = name
    row[_SB_HEADER.index("total_share")] = "1000000000"
    row[_SB_HEADER.index("industry")] = industry
    row[_SB_HEADER.index("region")] = "上海"
    row[_SB_HEADER.index("update_date")] = "2024-01-01"
    return row


class TestStockBasicIndustryEn:
    def test_mapped_and_unmapped_industries(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        mapping_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: mapped industry 白酒Ⅱ → 'Liquor'; unmapped 银行 → NULL."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setenv(_MAPPING_ENV, str(mapping_path))
        _write_csv(
            tmp_path / "stock_basic_official.csv",
            _SB_HEADER,
            [
                _sb_row("SH600519", "贵州茅台", "白酒Ⅱ"),
                _sb_row("SH600000", "平安银行", "银行"),
            ],
        )

        main_mod._import_stock_basic()

        # mapped industry → correct English name (acceptance #3)
        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COALESCE(industry_en, '<NULL>') FROM stock_basic "
                    "WHERE symbol='SH600519'"
                )
            )
            == "Liquor"
        )
        # unmapped industry → NULL (scenario 2)
        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COUNT(*) FROM stock_basic "
                    "WHERE symbol='SH600000' AND industry_en IS NULL"
                )
            )
            == "1"
        )

    def test_trimmed_industry_matches_mapping(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        mapping_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED (scenario 4): padded ' 白酒Ⅱ ' must TRIM-mach the '白酒Ⅱ' key."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setenv(_MAPPING_ENV, str(mapping_path))
        _write_csv(
            tmp_path / "stock_basic_official.csv",
            _SB_HEADER,
            [
                _sb_row("SH600519", "贵州茅台", " 白酒Ⅱ "),
            ],
        )

        main_mod._import_stock_basic()

        assert (
            _last(
                dolt_sql_csv(
                    "SELECT COALESCE(industry_en, '<NULL>') FROM stock_basic "
                    "WHERE symbol='SH600519'"
                )
            )
            == "Liquor"
        )
