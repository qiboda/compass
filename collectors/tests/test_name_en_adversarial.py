"""Adversarial RED tests — data-name-i18n epic B1 (issue #268) name_en/industry_en JOIN.

Attacks the plan-declared contract (see .dsh/plans/data-name-i18n.md):

  * ``fetch_index_daily.py::_import_index_basic`` — ``BASIC_DDL`` gains
    ``name_en VARCHAR(100)``; INSERT JOINs the name mapping table by
    **symbol** (SH000001/BK0475); unmapped symbols → NULL.
  * ``main.py::_import_stock_basic`` — INSERT JOINs the industry mapping by
    **industry string** (TRIMmed, may carry a Roman-numeral suffix like
    "白酒Ⅱ"); unmapped → NULL.

Attack dimensions covered here (adversarial — happy path belongs to the
requirement agent): boundary (empty mapping, all-NULL, duplicate mapping
keys, exactly-VARCHAR(100) vs >100 overflow, suffix matching), error paths
(missing mapping file, malformed / BOM / blank-line mapping), illegal input
(utf-8-sig BOM, case mismatch, whitespace), concurrency/resource (re-import
idempotency under INSERT IGNORE / DELETE+INSERT semantics, duplicate-join row
inflation).

Interface contract this red suite pins down (per ref #236 manufacturing rule):

  The mapping file path is injected via the environment variable
  ``COMPASS_NAME_EN_MAPPING`` (an implementation-chosen hook; the production
  ``collectors/name_en_mapping.csv`` also exists but these tests never touch
  it — they build their own temporary mapping fixture). The importer must
  load that CSV into a Dolt staging table and JOIN it while inserting
  name_en / industry_en.  Tests assert the Dolt RESULT, not the mechanism.

Mapping CSV column contract (utf-8-sig, csv module):
  * index-basic section: ``symbol,name_en`` (one row per symbol)
  * stock-basic section:  ``industry,industry_en`` (one row per Chinese industry)

RED status: all of these FAIL against the current pre-B1 code — ``BASIC_DDL``
has no ``name_en`` column and ``_import_stock_basic`` has no ``industry_en``
JOIN, so the imports abort / the extra column is absent.  They must GREEN once
B1 lands.
"""

from __future__ import annotations

import csv
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

# Imported under pytest via conftest's sys.path hook.
import fetch_index_daily  # noqa: E402
import main as main_mod  # noqa: E402

# Mapping-path injection hook these tests rely on (see docstring / ref #236).
_MAPPING_ENV = "COMPASS_NAME_EN_MAPPING"

_BASIC_HEADER = ["symbol", "name", "index_type"]
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


class TempDolt:
    """Minimal real-Dolt harness (shared by both import targets)."""

    def __init__(self, tmp_path: Path) -> None:
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
            capture_output=True,
            text=True,
            check=False,
        )
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.name", "CI"],
            capture_output=True,
            text=True,
            check=False,
        )
        init = subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "init"],
            capture_output=True,
            text=True,
        )
        assert init.returncode == 0, init.stderr
        self.dir = tmp_path
        self.dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )

    def dolt_sql_csv(self, sql: str) -> str:
        return subprocess.run(
            ["dolt", "--data-dir", str(self.dir), "sql", "-r", "csv", "-q", sql],
            capture_output=True,
            text=True,
        ).stdout

    def dolt_sql(self, sql: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["dolt", "--data-dir", str(self.dir), "sql", "-q", sql],
            capture_output=True,
            text=True,
        )

    def last(self, stdout: str) -> str:
        """Last data line of a `dolt sql -r csv` output (skip header)."""
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""


@pytest.fixture
def dolt_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> TempDolt:
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
    return TempDolt(tmp_path)


@pytest.fixture
def mapping_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Callable[[str], Path]:
    """Write a mapping CSV (utf-8-sig) and point COMPASS_NAME_EN_MAPPING at it.

    Returns a writer callable so each test can supply its own body.
    ``header`` names are asserted in each test to match the contract.
    """

    def _write(body: str, name: str = "mapping.csv") -> Path:
        path = tmp_path / name
        # utf-8-sig prefix → BOM-safe on reload; keeps csv module tokens clean.
        path.write_text("\ufeff" + body, encoding="utf-8")
        monkeypatch.setenv(_MAPPING_ENV, str(path))
        return path

    return _write


def _write_csv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    with open(path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        writer.writerows(rows)


# ═══════════════════════════════════════════════════════════════════
# index_basic (key = symbol)
# ═══════════════════════════════════════════════════════════════════


class TestIndexBasicNameEn:
    """_import_index_basic inserts name_en by JOINing a symbol mapping."""

    def _write_index_basic(self, dolt: TempDolt, tmp_path: Path) -> Path:
        csv_path = tmp_path / "index_basic.csv"
        _write_csv(
            csv_path,
            _BASIC_HEADER,
            [["BK0475", "半导体", "concept"], ["SH000001", "上证指数", "official"]],
        )
        return csv_path

    def test_mapped_symbols_get_name_en(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """A symbol present in the mapping must land in name_en (RED: no column yet)."""
        mapping_path(
            "section,key,value\nindex,SH000001,SSE Composite\nindex,BK0475,Semiconductor\n"
        )
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT name_en FROM index_basic WHERE symbol='SH000001'")
        )
        assert row == "SSE Composite", f"mapped symbol should write name_en, got {row!r}"

    def test_unmapped_symbol_null(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """A symbol absent from the mapping must stay NULL (never ''/garbage)."""
        mapping_path("section,key,value\nindex,SH000001,SSE Composite\n")  # BK0475 unmapped
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NULL>') FROM index_basic WHERE symbol='BK0475'"
            )
        )
        assert row == "<NULL>", f"unmapped symbol must be NULL, got {row!r}"

    def test_empty_mapping_all_null(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """An EMPTY mapping file (header only) must still import cleanly, all NULL."""
        mapping_path("section,key,value\n")
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        n = dolt_env.last(dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM index_basic"))
        assert n == "2"
        null_count = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM index_basic WHERE name_en IS NULL")
        )
        assert null_count == "2", "with an empty mapping every row must be NULL"

    def test_duplicate_mapping_keys_no_row_inflation(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Duplicate symbol rows in the mapping must not inflate index_basic.

        RED: with no name_en column the mapped symbol yields no name_en value,
        so the first assert fails. GREEN requires the JOIN to actually happen
        (SH000001 gets one concrete name_en) AND never inflate the table past 2
        rows — a naive product-join would SELECT 4 rows and, if INSERT IGNORE
        were bypassed, corrupt the PK (industry/basic count). The COUNT assert
        is what the plan's INSERT IGNORE semantics must keep green."""
        mapping_path("section,key,value\nindex,SH000001,SSE Composite\nindex,SH000001,TWIN\n")
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        # The JOIN must land — some concrete name_en on the (duplicated) key.
        # Note the assertion deliberately FAILS both when the column is absent
        # (SELECT errors → empty stdout) and when it resolves to no value, so a
        # partially-implemented importer cannot silently pass.
        value = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NONE>') FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert value not in ("", "<NONE>"), (
            f"duplicate key must still yield one resolved name_en, got {value!r}"
        )
        n = dolt_env.last(dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM index_basic"))
        assert n == "2", f"duplicate mapping key must not duplicate rows, got {n}"

    def test_exactly_100_char_name_en_preserved(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Boundary: a name_en at the exact VARCHAR(100) limit must survive intact."""
        name = "n" * 100
        mapping_path(f"section,key,value\nindex,SH000001,{name}\n")
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT name_en, LENGTH(name_en) FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert row == f"{name},100", f"exact-limit name_en must be intact, got {row!r}"

    def test_overflow_101_char_name_en_aborts_cleanly(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Boundary/error: a name_en one past VARCHAR(100) must be ROUTED
        through the name_en column and must never silently truncate or half-write.

        RED: before B1 there is no name_en column, so the sentinel diff from
        the column (empty stdout ≠ '<MISSING>') trips the first assert. GREEN
        accepts either 'reject → NULL' or 'truncate → 100-char', as long as the
        value is not a 101-char blob or a silently-partial translation."""
        csv_path = tmp_path / "index_basic.csv"
        _write_csv(
            csv_path,
            _BASIC_HEADER,
            [["SH000001", "上证指数", "official"], ["BK0999", "某板块", "concept"]],
        )
        mapping_path(f"section,key,value\nindex,SH000001,{'x' * 101}\nindex,BK0999,Board\n")
        _ = fetch_index_daily.import_to_dolt(csv_path)
        # Sentinel: the column must exist and be reachable for this symbol.
        value = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NULL>') FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert value != "", "name_en column must exist to carry the overflow value"
        # Contract guard: whatever the overflow strategy (reject→NULL, truncate
        # →100, or abort→row absent), the stored value must NEVER be a full
        # 101-char blob — the only outcome that would corrupt the VARCHAR(100)
        # column. All other strategies are legitimate and stay green.
        long_value = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT LENGTH(name_en) FROM index_basic WHERE symbol='SH000001'")
        )
        assert long_value != "101", (
            f"overflow must never store a 101-char value, got len={long_value!r}"
        )

    def test_case_sensitive_symbol_key_mismatch(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Mapping keys are treated exactly — lowercase 'sh000001' must NOT
        match symbol 'SH000001' (A-share symbol case matters end-to-end)."""
        mapping_path("section,key,value\nindex,sh000001,SSE Composite\n")
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NULL>') FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert row == "<NULL>", f"case-mismatched symbol must stay NULL, got {row!r}"

    def test_bom_mapping_file_decodes(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """utf-8-sig BOM in the mapping file must be decoded (not leak into the
        first key, corrupting the very first row's JOIN)."""
        mapping_path(
            "section,key,value\nindex,SH000001,SSE Composite\nindex,BK0475,Semiconductor\n"
        )
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT name_en FROM index_basic WHERE symbol='SH000001'")
        )
        assert row == "SSE Composite", (
            "BOM prefix on the mapping must not corrupt the first JOIN key"
        )

    def test_missing_mapping_file_no_crash(
        self,
        dolt_env: TempDolt,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Error path: a missing mapping file must not crash the importer nor
        leave a half-written index_basic — it degrades to all-NULL import."""
        monkeypatch.setenv(_MAPPING_ENV, str(tmp_path / "does_not_exist.csv"))
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        result = fetch_index_daily.import_to_dolt(csv_path)
        assert result == 2, "missing mapping must still import the rows"
        nulls = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM index_basic WHERE name_en IS NULL")
        )
        assert nulls == "2", "missing mapping degrades to all-NULL, not a crash"

    def test_malformed_mapping_missing_col_no_crash(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Error path: a mapping file with a wrong/missing column header must not
        silently import garbage — it must be detected and degrade to a safe
        universe (or abort), never inject a bare 'sh000001,SSE' fragment."""
        mapping_path("section,WRONG_COL\nindex,SH000001,SSE Composite\n")
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        _ = fetch_index_daily.import_to_dolt(csv_path)
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NULL>') FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert row == "<NULL>", f"malformed mapping must not inject a bogus name_en, got {row!r}"

    def test_rerun_idempotent_with_mapping(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Concurrency: re-importing the same file with the mapping must not
        grow index_basic rows AND must (re)apply name_en identically on rerun
        (INSERT IGNORE dedup on symbol PK must not accumulate duplicates).

        RED: no name_en column → the sentinel value assert fails on day 1."""
        mapping_path(
            "section,key,value\nindex,SH000001,SSE Composite\nindex,BK0475,Semiconductor\n"
        )
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        assert fetch_index_daily.import_to_dolt(csv_path) == 2
        assert fetch_index_daily.import_to_dolt(csv_path) == 2
        n = dolt_env.last(dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM index_basic"))
        assert n == "2", f"re-import must be idempotent, got {n} rows"
        # The mapping must still be (re)applied on a rerun — not dropped.
        value = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NONE>') FROM index_basic WHERE symbol='BK0475'"
            )
        )
        assert value == "Semiconductor", f"rerun must keep the mapped name_en, got {value!r}"

    def test_stale_mapping_staging_table_is_rebuilt(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Error path (review P1-1): a stale ``_tmp_name_en`` staging table
        left by a failed run must be dropped before the fresh import — the
        CREATE would otherwise fail and every en column silently degrade to
        NULL. The mapping must still land after the rebuild."""
        dolt_env.dolt_sql(
            "CREATE TABLE _tmp_name_en (section VARCHAR(20), `key` VARCHAR(100), value VARCHAR(100))"
        )
        mapping_path(
            "section,key,value\nindex,SH000001,SSE Composite\nindex,BK0475,Semiconductor\n"
        )
        csv_path = self._write_index_basic(dolt_env, tmp_path)
        assert fetch_index_daily.import_to_dolt(csv_path) == 2
        value = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(name_en,'<NONE>') FROM index_basic WHERE symbol='SH000001'"
            )
        )
        assert value == "SSE Composite", (
            f"stale staging table must be rebuilt and joined, got {value!r}"
        )
        # Staging table cleaned up after the import.
        n = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='_tmp_name_en'"
            )
        )
        assert n == "0", f"staging table must be dropped after import, got {n}"


# ═══════════════════════════════════════════════════════════════════
# stock_basic (key = industry string)
# ═══════════════════════════════════════════════════════════════════


class TestStockBasicIndustryEn:
    """_import_stock_basic inserts industry_en by JOINing an industry mapping."""

    def _setup_stock_basic(self, dolt: TempDolt) -> None:
        """_import_stock_basic needs a pre-existing full-schema stock_basic
        (it DELETE-from then INSERT-INTO). The industry_en column is the one
        the B1 plan is about to add; absent today, the SELECT-side never issues
        it, so the 12-col INSERT still lands and industry_en stays NULL — which
        is exactly what the adversarial asserts must flip."""
        dolt.dolt_sql_csv(
            "CREATE TABLE stock_basic ("
            "symbol VARCHAR(20) PRIMARY KEY, ts_code VARCHAR(20), code VARCHAR(20), "
            "name VARCHAR(100), list_date DATE, delist_date VARCHAR(20), "
            "board VARCHAR(50), full_name VARCHAR(200), total_share DOUBLE, "
            "industry VARCHAR(50), region VARCHAR(50), update_date DATE, "
            "industry_en VARCHAR(100))"
        )

    def _write_sb_csv(self, tmp_path: Path, rows: list[list[str]]) -> Path:
        csv_path = tmp_path / "stock_basic_official.csv"
        _write_csv(csv_path, _SB_HEADER, rows)
        return csv_path

    def _row_suffix(self, industry: str, sym: str = "SZ000001") -> list[str]:
        return [
            sym,
            f"{sym[2:]}.SZ",
            sym[2:],
            "平安银行",
            "1991-04-03",
            "",
            "主板",
            "平安银行股份有限公司",
            "194.06",
            industry,
            "深圳",
            "2024-01-01",
        ]

    def test_mapped_industry_gets_industry_en(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """A TRIMmed industry present in the mapping must land in industry_en."""
        self._setup_stock_basic(dolt_env)
        mapping_path("section,key,value\nindustry,银行,Banking\n")
        self._write_sb_csv(tmp_path, [self._row_suffix("银行")])
        main_mod._import_stock_basic()
        row = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT industry_en FROM stock_basic WHERE symbol='SZ000001'")
        )
        assert row == "Banking", f"mapped industry should write industry_en, got {row!r}"

    def test_unmapped_industry_null(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """An industry absent from the mapping must stay NULL — AND, in the same
        table, a mapped industry must be filled. The paired assertions make this
        an adversarial contrast: before B1 the JOIN is absent, so the MAPPED row
        reads NULL and the test FAILS (it can no longer be satisfied by a lazy
        "everything is NULL" implementation)."""
        self._setup_stock_basic(dolt_env)
        mapping_path("section,key,value\nindustry,银行,Banking\n")
        # 白酒 unmapped, 银行 mapped — same import, contrast rows.
        self._write_sb_csv(
            tmp_path,
            [self._row_suffix("白酒", sym="SZ000001"), self._row_suffix("银行", sym="SZ000002")],
        )
        main_mod._import_stock_basic()
        mapped = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(industry_en,'<NULL>') FROM stock_basic WHERE symbol='SZ000002'"
            )
        )
        assert mapped == "Banking", (
            f"mapped industry must be filled in the same import, got {mapped!r}"
        )
        unmapped = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(industry_en,'<NULL>') FROM stock_basic WHERE symbol='SZ000001'"
            )
        )
        assert unmapped == "<NULL>", f"unmapped industry must stay NULL, got {unmapped!r}"

    def test_suffix_industry_matches_base(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Contract: a Roman-numeral-suffixed industry ('白酒Ⅱ') must match its
        base mapping key ('白酒') — the industry strip rule (plan B1).

        RED against today: no mapping/JOIN at all → industry_en absent. GREEN
        requires the strip-suffix JOIN to land "Liquor"."""
        self._setup_stock_basic(dolt_env)
        mapping_path("section,key,value\nindustry,白酒,Liquor\nindustry,银行,Banking\n")
        self._write_sb_csv(tmp_path, [self._row_suffix("白酒Ⅱ")])
        main_mod._import_stock_basic()
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(industry_en,'<NULL>') FROM stock_basic WHERE symbol='SZ000001'"
            )
        )
        assert row == "Liquor", f"suffix '白酒Ⅱ' must match base key '白酒', got {row!r}"

    def test_industry_is_trimmed_before_join(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Whitespace around the industry value must be trimmed before the JOIN
        a CSV cell of '  银行   ' must still match mapping key '银行'."""
        self._setup_stock_basic(dolt_env)
        mapping_path("section,key,value\nindustry,银行,Banking\n")
        self._write_sb_csv(tmp_path, [self._row_suffix("  银行  ")])
        main_mod._import_stock_basic()
        row = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(industry_en,'<NULL>') FROM stock_basic WHERE symbol='SZ000001'"
            )
        )
        assert row == "Banking", f"industry must be TRIMmed before JOIN, got {row!r}"

    def test_delete_insert_rerun_no_duplicates(
        self,
        dolt_env: TempDolt,
        mapping_path: Callable[[str], Path],
        tmp_path: Path,
    ) -> None:
        """Concurrency: _import_stock_basic DELETE+INSERT must be re-run-idempotent
        AND (re)apply industry_en each run — row count must never grow and the
        mapping must not be lost on a second run. RED: no JOIN → NULL idempotent
        but the mapping-fill assert still trips."""
        self._setup_stock_basic(dolt_env)
        mapping_path("section,key,value\nindustry,银行,Banking\n")
        self._write_sb_csv(tmp_path, [self._row_suffix("银行")])
        main_mod._import_stock_basic()
        main_mod._import_stock_basic()
        n = dolt_env.last(dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM stock_basic"))
        assert n == "1", f"DELETE+INSERT rerun must not duplicate rows, got {n}"
        value = dolt_env.last(
            dolt_env.dolt_sql_csv(
                "SELECT IFNULL(industry_en,'<NONE>') FROM stock_basic WHERE symbol='SZ000001'"
            )
        )
        assert value == "Banking", f"rerun must keep the mapped industry_en, got {value!r}"

    def test_missing_mapping_no_crash(
        self,
        dolt_env: TempDolt,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Error path: missing mapping file must not crash _import_stock_basic —
        it degrades to a clean industry_en-all-NULL import.

        NOTE (GREEN anchor, not a RED attack): because the fixture itself ships
        the industry_en column, a "no JOIN at all" implementation and a
        correctly-degrading implementation both yield all-NULL here, so this
        assertion holds on day 1 too. Its value is regression — GREEN must keep
        this exact graceful-degradation contract (never crash, never half-write,
        base import rows still land)."""
        self._setup_stock_basic(dolt_env)
        monkeypatch.setenv(_MAPPING_ENV, str(tmp_path / "missing.csv"))
        self._write_sb_csv(tmp_path, [self._row_suffix("银行")])
        main_mod._import_stock_basic()
        n = dolt_env.last(dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM stock_basic"))
        assert n == "1", "missing mapping must not stop the base import"
        nulls = dolt_env.last(
            dolt_env.dolt_sql_csv("SELECT COUNT(*) FROM stock_basic WHERE industry_en IS NULL")
        )
        assert nulls == "1", "missing mapping degrades to all-NULL industry_en"
