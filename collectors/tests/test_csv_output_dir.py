"""Issue #208 — unified raw-CSV output directory for all collectors.

Contract under test (ref #208):
1. ``common.csv_dir()`` resolves ``COMPASS_CSV_DIR`` (default
   ``/data/compass-data/csv``) — unit tests live in
   ``test_common.py::TestCsvDir``.
2. Every collector's fetch main flow defaults its output path to
   ``csv_dir() / f"{REPORT_NAME}.csv"`` — no more relative CSVs in cwd.
3. Explicit ``-o/--output`` still overrides the default directory.
4. ``main.py`` import helpers read the CSVs from ``csv_dir()``.

RED phase: ``csv_dir()`` does not exist yet and every default output path
is still relative to cwd — all "default path" tests below fail; the
``-o/--output`` override pins already pass (regression guards).
"""

import asyncio
import importlib
import sys
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import SyncStubSession  # noqa: E402


class TestRunDefaultOutputInCsvDir:
    """Every run()-based collector defaults output_path to csv_dir()/REPORT_NAME.csv.

    The since-based collectors short-circuit through the existing "no new
    periods" early-return (last_report_date patched to a future date), so
    run() returns its output_path without touching the network or a session
    stub. The returned Path is exactly the path run() writes to.
    """

    @pytest.mark.parametrize(
        "module_name, report_name, run_kwargs, since_value",
        [
            pytest.param(
                "fetch_income", "RPT_DMSK_FN_INCOME",
                {"years": [2024], "periods": "FY"}, "2099-12-31",
                id="income",
            ),
            pytest.param(
                "fetch_balance_sheet", "RPT_DMSK_FN_BALANCE",
                {"years": [2024], "periods": "FY"}, "2099-12-31",
                id="balance_sheet",
            ),
            pytest.param(
                "fetch_cash_flow", "RPT_DMSK_FN_CASHFLOW",
                {"years": [2024], "periods": "FY"}, "2099-12-31",
                id="cash_flow",
            ),
            pytest.param(
                "fetch_block_trade", "RPT_DATA_BLOCKTRADE",
                {"years": [2024]}, "2099-12-31",
                id="block_trade",
            ),
            pytest.param(
                "fetch_dragon", "RPT_DAILYBILLBOARD_DETAILSNEW",
                {}, "2099-12-31",
                id="dragon",
            ),
            pytest.param(
                "fetch_institution_survey", "RPT_ORG_SURVEYNEW",
                {}, "2099-12-31",
                id="institution_survey",
            ),
            pytest.param(
                "fetch_main_flow", "RPT_MAIN_MONEY_FLOW",
                {}, "today",
                id="main_flow",
            ),
        ],
    )
    async def test_default_output_path_in_csv_dir(
        self,
        monkeypatch,
        tmp_path: Path,
        module_name: str,
        report_name: str,
        run_kwargs: dict,
        since_value: str,
    ) -> None:
        """run() returns csv_dir()/REPORT_NAME.csv when no explicit output is given."""
        mod = importlib.import_module(module_name)
        csv_dir = tmp_path / "csv"
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        if since_value == "today":
            # main_flow short-circuits only when the watermark equals today.
            monkeypatch.setattr(
                mod, "last_report_date", lambda _tbl: mod._today().isoformat()
            )
        else:
            monkeypatch.setattr(mod, "last_report_date", lambda _tbl: since_value)

        result = await mod.run(**run_kwargs)

        assert result == csv_dir / f"{report_name}.csv"

    async def test_concept_member_default_output_in_csv_dir(
        self, make_stub_session, monkeypatch, tmp_path: Path
    ) -> None:
        """concept_member has no watermark check — full run with a stubbed session.

        One board with zero members makes write_csv([]) a no-op, and run()
        returns its output_path without writing anything.
        """
        import fetch_concept_member as mod

        csv_dir = tmp_path / "csv"
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            canned_responses={
                mod.BOARD_LIST_URL: {
                    "json_data": {
                        "data": {"total": 1, "diff": [{"f12": "BK0001", "f14": "测试板块"}]},
                    }
                },
                mod.EM_BASE: {
                    "json_data": {"success": True, "result": {"data": [], "pages": 1}},
                },
            }
        )
        with patch("fetch_concept_member.AsyncSession", return_value=stub):
            result = await mod.run(page_size=100)

        assert result == csv_dir / f"{mod.REPORT_NAME}.csv"


class TestArgparseDefaultOutputInCsvDir:
    """argparse-based collectors: default output (no -o/--output) lands in csv_dir()."""

    def test_stock_basic_official_default_output_in_csv_dir(
        self, tmp_path: Path, monkeypatch
    ) -> None:
        import fetch_stock_basic_official as fsbo

        csv_dir = tmp_path / "csv"
        csv_dir.mkdir(parents=True)
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        scratch = tmp_path / "cwd"
        scratch.mkdir()
        monkeypatch.chdir(scratch)

        # All exchanges fail — main() still writes the header-only CSV.
        stub = SyncStubSession()

        def _boom(*args, **kwargs):  # noqa: ANN002, ANN003
            raise RuntimeError("simulated exchange failure")

        stub.get = _boom  # type: ignore[method-assign]
        stub.post = _boom  # type: ignore[method-assign]
        monkeypatch.setattr(fsbo.requests, "Session", lambda: stub)
        monkeypatch.setattr(fsbo.time, "sleep", Mock())
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "--update-date", "2026-07-31"],
        )

        fsbo.main()

        out = csv_dir / "stock_basic_official.csv"
        assert out.exists()
        assert not (scratch / "stock_basic_official.csv").exists()

    async def test_stock_basic_default_output_in_csv_dir(
        self, make_stub_session, monkeypatch, tmp_path: Path
    ) -> None:
        import fetch_stock_basic as fsb

        csv_dir = tmp_path / "csv"
        csv_dir.mkdir(parents=True)
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        scratch = tmp_path / "cwd"
        scratch.mkdir()
        monkeypatch.chdir(scratch)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        # total=0 → no pages fetched → the file is created (empty) at output_path.
        stub = make_stub_session(json_data={"data": {}})
        with (
            patch("fetch_stock_basic.AsyncSession", return_value=stub),
            patch.object(fsb.sys, "argv", ["fetch_stock_basic.py"]),
        ):
            await fsb.main()

        out = csv_dir / "stock_basic.csv"
        assert out.exists()
        assert not (scratch / "stock_basic.csv").exists()

    async def test_fin_indicators_default_output_in_csv_dir(
        self, make_stub_session, monkeypatch, tmp_path: Path
    ) -> None:
        import fetch_fin_indicators as ffi

        csv_dir = tmp_path / "csv"
        csv_dir.mkdir(parents=True)
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        scratch = tmp_path / "cwd"
        scratch.mkdir()
        monkeypatch.chdir(scratch)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )
        with (
            patch("fetch_fin_indicators.AsyncSession", return_value=stub),
            patch.object(
                ffi.sys, "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await ffi.main()

        out = csv_dir / "RPT_LICO_FN_CPD.csv"
        assert out.exists()
        assert not (scratch / "RPT_LICO_FN_CPD.csv").exists()


class TestOutputOverrideWins:
    """-o/--output must keep overriding the default csv_dir() — no regression."""

    def test_stock_basic_official_output_flag_overrides(
        self, tmp_path: Path, monkeypatch
    ) -> None:
        import fetch_stock_basic_official as fsbo

        csv_dir = tmp_path / "csv"
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        monkeypatch.chdir(tmp_path)

        stub = SyncStubSession()

        def _boom(*args, **kwargs):  # noqa: ANN002, ANN003
            raise RuntimeError("simulated exchange failure")

        stub.get = _boom  # type: ignore[method-assign]
        stub.post = _boom  # type: ignore[method-assign]
        monkeypatch.setattr(fsbo.requests, "Session", lambda: stub)
        monkeypatch.setattr(fsbo.time, "sleep", Mock())
        override = tmp_path / "override.csv"
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "-o", str(override),
             "--update-date", "2026-07-31"],
        )

        fsbo.main()

        assert override.exists()
        assert not (csv_dir / "stock_basic_official.csv").exists()

    async def test_fin_indicators_output_flag_overrides(
        self, make_stub_session, monkeypatch, tmp_path: Path
    ) -> None:
        import fetch_fin_indicators as ffi

        csv_dir = tmp_path / "csv"
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )
        override = tmp_path / "override.csv"
        with (
            patch("fetch_fin_indicators.AsyncSession", return_value=stub),
            patch.object(
                ffi.sys, "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY",
                 "--output", str(override)],
            ),
        ):
            await ffi.main()

        assert override.exists()
        assert not (csv_dir / "RPT_LICO_FN_CPD.csv").exists()


class TestMainImportReadsCsvDir:
    """main.py import helpers must read CSVs from csv_dir(), not COLLECTORS_DIR."""

    def test_import_stock_basic_reads_csv_dir(
        self, monkeypatch, tmp_path: Path
    ) -> None:
        import common
        import main as main_mod

        csv_dir = tmp_path / "csv"
        csv_dir.mkdir(parents=True)
        (csv_dir / "stock_basic_official.csv").write_text("header\n1\n")
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))

        mock_sql = Mock(return_value=Mock(stdout="Count\n100", returncode=0))
        monkeypatch.setattr(common, "dolt_sql", mock_sql)
        mock_sql_csv = Mock(return_value="Count\n100")
        monkeypatch.setattr(common, "dolt_sql_csv", mock_sql_csv)
        mock_table_import = Mock(return_value=True)
        monkeypatch.setattr(common, "dolt_table_import", mock_table_import)

        main_mod._import_stock_basic()

        assert mock_table_import.call_args.args[1] == csv_dir / "stock_basic_official.csv"

    def test_import_fin_indicators_reads_csv_dir(
        self, monkeypatch, tmp_path: Path
    ) -> None:
        import common
        import main as main_mod

        csv_dir = tmp_path / "csv"
        csv_dir.mkdir(parents=True)
        (csv_dir / "RPT_LICO_FN_CPD.csv").write_text("SECUCODE,SECURITY_CODE\n000001.SZ,000001\n")
        monkeypatch.setenv("COMPASS_CSV_DIR", str(csv_dir))

        mock_irt = Mock(return_value=0)
        monkeypatch.setattr(common, "import_replace_table", mock_irt)

        main_mod._import_fin_indicators()

        assert mock_irt.call_args.kwargs["csv_path"] == csv_dir / "RPT_LICO_FN_CPD.csv"
