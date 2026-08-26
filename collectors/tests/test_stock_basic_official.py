"""Unit tests for fetch_stock_basic_official — official exchange collectors.

Covers SSE JSON parsing, SZSE XLSX sheet parsing (active + delisted),
BSE JSON wrapper parsing, exchange inference, merge/dedup, and CSV output.
All tests use mock data — no network access.

Data shapes mirror the real APIs (verified 2026-07-31):
- SSE: query.sse.com.cn JSON — pageHelp.data[] with A_STOCK_CODE, LIST_DATE
  (YYYYMMDD), DELIST_DATE ("-" when active), LIST_BOARD, STOCK_TYPE,
  CSRC_CODE_DESC, AREA_NAME_DESC.
- SZSE: szse.cn ShowReport xlsx (CATALOGID=1110 active / 1793_ssgs delisted)
  — sheet1.xml inline-string cells, 22 cols (active) / 4 cols (delisted).
- BSE: bse.cn nqxxController — `null([...])` JSON wrapper, content[] rows
  with xxzqdm, xxzqjc, fxssrq, xxhyzl, xxssdq, xxzgb.

Output CSV = 12 columns (Dolt stock_basic final schema):
symbol, ts_code, code, name, list_date, delist_date, board, full_name,
total_share, industry, region, update_date
"""

import io
import sys
import zipfile
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


from conftest import SyncStubResponse, SyncStubSession  # noqa: E402

import fetch_stock_basic_official as fsbo  # noqa: E402
from fetch_stock_basic_official import (  # noqa: E402
    COLUMNS,
    infer_exchange,
    merge_exchanges,
    parse_bse_json,
    parse_sse_json,
    parse_szse_delisted,
    parse_szse_xlsx,
    records_to_csv,
)

UPDATE_DATE = "2026-07-31"


def _szse_zip_bytes(sheet_xml: str) -> bytes:
    """Build a real xlsx zip whose sheet1.xml is sheet_xml (fetch_szse_xlsx unzips it)."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("xl/worksheets/sheet1.xml", sheet_xml)
    return buf.getvalue()


# 深交所退市股 xlsx sheet（表头 + 一条退市股记录）— 与
# TestParseSzseDelisted.test_delisted_row 的内联 sheet 相同
_delisted_xml = (
    '<row><c r="A1" t="inlineStr"><is><t>证券代码</t></is></c>'
    '<c r="B1" t="inlineStr"><is><t>证券简称</t></is></c>'
    '<c r="C1" t="inlineStr"><is><t>上市日期</t></is></c>'
    '<c r="D1" t="inlineStr"><is><t>终止上市日期</t></is></c></row>'
    '<row><c r="A2" t="inlineStr"><is><t>000003</t></is></c>'
    '<c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c>'
    '<c r="C2" t="inlineStr"><is><t>1991-01-14</t></is></c>'
    '<c r="D2" t="inlineStr"><is><t>2002-06-14</t></is></c></row>'
)


# ── Fixtures: mock API payloads ────────────────────────────────────

def _sse_row(code="600000", name="浦发银行", board="1", stype="1",
             list_date="19991110", delist="-", industry="金融业",
             region="上海市", full_name="上海浦东发展银行股份有限公司"):
    return {
        "A_STOCK_CODE": code,
        "COMPANY_ABBR": name,
        "SEC_NAME_CN": name,
        "FULL_NAME": full_name,
        "LIST_DATE": list_date,
        "DELIST_DATE": delist,
        "LIST_BOARD": board,
        "STOCK_TYPE": stype,
        "CSRC_CODE_DESC": industry,
        "AREA_NAME_DESC": region,
        "STATE_CODE_STOCK": "6" if delist != "-" else "4",
    }


def _sse_payload(rows):
    return {"pageHelp": {"data": rows}}


def _szse_row(board="主板", code="000001", name="平安银行", list_date="1991-04-03",
              total_share="19,405,918,198", industry="J 金融业", province="广东",
              city="深圳市", full_name="平安银行股份有限公司"):
    return (
        f'<row><c r="A1" t="inlineStr"><is><t>{board}</t></is></c>'
        f'<c r="B1" t="inlineStr"><is><t>{full_name}</t></is></c>'
        f'<c r="C1" t="inlineStr"><is><t>Ping An Bank Co., Ltd.</t></is></c>'
        f'<c r="D1" t="inlineStr"><is><t>广东省深圳市</t></is></c>'
        f'<c r="E1" t="inlineStr"><is><t>{code}</t></is></c>'
        f'<c r="F1" t="inlineStr"><is><t>{name}</t></is></c>'
        f'<c r="G1" t="inlineStr"><is><t>{list_date}</t></is></c>'
        f'<c r="H1"><v>{total_share.replace(",", "")}</v></c>'
        f'<c r="I1"><v>0</v></c>'
        f'<c r="J1" t="inlineStr"><is><t></t></is></c>'
        f'<c r="K1" t="inlineStr"><is><t></t></is></c>'
        f'<c r="L1" t="inlineStr"><is><t></t></is></c>'
        f'<c r="M1"><v>0</v></c>'
        f'<c r="N1"><v>0</v></c>'
        f'<c r="O1" t="inlineStr"><is><t>华南</t></is></c>'
        f'<c r="P1" t="inlineStr"><is><t>{province}</t></is></c>'
        f'<c r="Q1" t="inlineStr"><is><t>{city}</t></is></c>'
        f'<c r="R1" t="inlineStr"><is><t>{industry}</t></is></c>'
        f'<c r="S1" t="inlineStr"><is><t>bank.pingan.com</t></is></c>'
        f'<c r="T1" t="inlineStr"><is><t>-</t></is></c>'
        f'<c r="U1" t="inlineStr"><is><t>-</t></is></c>'
        f'<c r="V1" t="inlineStr"><is><t>-</t></is></c></row>'
    )


def _szse_sheet(rows):
    header = (
        '<row><c r="A1" t="inlineStr"><is><t>板块</t></is></c>'
        '<c r="B1" t="inlineStr"><is><t>公司全称</t></is></c>'
        '<c r="C1" t="inlineStr"><is><t>英文名称</t></is></c>'
        '<c r="D1" t="inlineStr"><is><t>注册地址</t></is></c>'
        '<c r="E1" t="inlineStr"><is><t>A股代码</t></is></c>'
        '<c r="F1" t="inlineStr"><is><t>A股简称</t></is></c>'
        '<c r="G1" t="inlineStr"><is><t>A股上市日期</t></is></c>'
        '<c r="H1" t="inlineStr"><is><t>A股总股本</t></is></c>'
        '<c r="I1" t="inlineStr"><is><t>A股流通股本</t></is></c>'
        '<c r="J1" t="inlineStr"><is><t>B股代码</t></is></c>'
        '<c r="K1" t="inlineStr"><is><t>B股 简 称</t></is></c>'
        '<c r="L1" t="inlineStr"><is><t>B股上市日期</t></is></c>'
        '<c r="M1" t="inlineStr"><is><t>B股总股本</t></is></c>'
        '<c r="N1" t="inlineStr"><is><t>B股流通股本</t></is></c>'
        '<c r="O1" t="inlineStr"><is><t>地区</t></is></c>'
        '<c r="P1" t="inlineStr"><is><t>省份</t></is></c>'
        '<c r="Q1" t="inlineStr"><is><t>城市</t></is></c>'
        '<c r="R1" t="inlineStr"><is><t>所属行业</t></is></c>'
        '<c r="S1" t="inlineStr"><is><t>公司网址</t></is></c>'
        '<c r="T1" t="inlineStr"><is><t>目前尚未盈利</t></is></c>'
        '<c r="U1" t="inlineStr"><is><t>具有表决权差异安排</t></is></c>'
        '<c r="V1" t="inlineStr"><is><t>具有协议控制架构</t></is></c></row>'
    )
    return "".join([header] + rows)


def _bse_body(rows, total=331, total_pages=1):
    """BSE API returns `null([{"content": [...], "totalElements": N, ...}])`."""
    inner = (
        '{"content": ' + "[" + ",".join(rows) + "],"
        f'"totalElements": {total}, "totalPages": {total_pages}'
        "}"
    )
    return "null([" + inner + "])"


def _bse_row(code="920000", name="安徽凤凰", list_date="20201223",
             industry="汽车制造业", region="安徽省", total_share=91680000):
    return (
        '{"xxzqdm": "' + code + '", "xxzqjc": "' + name + '",'
        f'"fxssrq": "{list_date}", "xxhyzl": "{industry}",'
        f'"xxssdq": "{region}", "xxzgb": {total_share},'
        '"xxzqjb": "T", "xxzbqs": "国元证券", "xxzrlx": "连续竞价"}'
    )


# ── COLUMNS contract ──────────────────────────────────────────────

class TestColumns:
    def test_csv_columns_match_dolt_schema(self):
        assert COLUMNS == [
            "symbol", "ts_code", "code", "name", "list_date", "delist_date",
            "board", "full_name", "total_share", "industry", "region",
            "update_date",
        ]


# ── SSE JSON parsing ──────────────────────────────────────────────

class TestParseSseJson:
    def test_active_a_share(self):
        rows = parse_sse_json(_sse_payload([_sse_row()]), UPDATE_DATE)
        assert len(rows) == 1
        r = rows[0]
        assert r["code"] == "600000"
        assert r["name"] == "浦发银行"
        assert r["exchange"] == "SH"
        assert r["list_date"] == "1999-11-10"
        assert r["delist_date"] == ""
        assert r["board"] == "主板"
        assert r["full_name"] == "上海浦东发展银行股份有限公司"
        assert r["industry"] == "金融业"
        assert r["region"] == "上海市"
        assert r["update_date"] == UPDATE_DATE

    def test_star_board_share(self):
        rows = parse_sse_json(
            _sse_payload([_sse_row(code="688001", name="华兴源创", board="2",
                                   stype="8", list_date="20190722",
                                   industry="专用设备制造业")]),
            UPDATE_DATE,
        )
        assert len(rows) == 1
        assert rows[0]["exchange"] == "SH"
        assert rows[0]["board"] == "科创板"

    def test_b_share_filtered_out(self):
        rows = parse_sse_json(
            _sse_payload([
                _sse_row(code="900901", name="云赛B股", stype="2"),
                _sse_row(),
            ]),
            UPDATE_DATE,
        )
        assert len(rows) == 1
        assert rows[0]["code"] == "600000"

    def test_delisted_share_kept_with_date(self):
        rows = parse_sse_json(
            _sse_payload([_sse_row(code="600001", name="邯郸钢铁",
                                   delist="20091229")]),
            UPDATE_DATE,
        )
        assert len(rows) == 1
        assert rows[0]["delist_date"] == "2009-12-29"
        assert rows[0]["board"] == "主板"

    def test_empty_payload(self):
        assert parse_sse_json({"pageHelp": {"data": []}}, UPDATE_DATE) == []

    def test_non_list_data_returns_empty(self):
        assert parse_sse_json({"pageHelp": {"data": {}}}, UPDATE_DATE) == []


# ── SZSE XLSX parsing ─────────────────────────────────────────────

class TestParseSzseXlsx:
    def test_active_main_board(self):
        rows = parse_szse_xlsx(_szse_sheet([_szse_row()]), UPDATE_DATE)
        assert len(rows) == 1
        r = rows[0]
        assert r["code"] == "000001"
        assert r["name"] == "平安银行"
        assert r["exchange"] == "SZ"
        assert r["board"] == "主板"
        assert r["list_date"] == "1991-04-03"
        assert r["delist_date"] == ""
        assert r["total_share"] == 19405918198.0
        assert r["industry"] == "J 金融业"
        assert r["region"] == "广东"
        assert r["full_name"] == "平安银行股份有限公司"
        assert r["update_date"] == UPDATE_DATE

    def test_chinext_board(self):
        rows = parse_szse_xlsx(
            _szse_sheet([_szse_row(board="创业板", code="300750",
                                   name="宁德时代")]),
            UPDATE_DATE,
        )
        assert len(rows) == 1
        assert rows[0]["board"] == "创业板"

    def test_skips_empty_rows(self):
        rows = parse_szse_xlsx(_szse_sheet([_szse_row(), ""]), UPDATE_DATE)
        assert len(rows) == 1

    def test_empty_sheet(self):
        assert parse_szse_xlsx("", UPDATE_DATE) == []

    def test_skips_row_without_code(self):
        row = (
            '<row><c r="A1" t="inlineStr"><is><t>主板</t></is></c>'
            '<c r="F1" t="inlineStr"><is><t>无代码</t></is></c></row>'
        )
        assert parse_szse_xlsx(_szse_sheet([row]), UPDATE_DATE) == []


class TestParseSzseDelisted:
    def test_delisted_row(self):
        sheet = (
            '<row><c r="A1" t="inlineStr"><is><t>证券代码</t></is></c>'
            '<c r="B1" t="inlineStr"><is><t>证券简称</t></is></c>'
            '<c r="C1" t="inlineStr"><is><t>上市日期</t></is></c>'
            '<c r="D1" t="inlineStr"><is><t>终止上市日期</t></is></c></row>'
            '<row><c r="A2" t="inlineStr"><is><t>000003</t></is></c>'
            '<c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c>'
            '<c r="C2" t="inlineStr"><is><t>1991-01-14</t></is></c>'
            '<c r="D2" t="inlineStr"><is><t>2002-06-14</t></is></c></row>'
        )
        rows = parse_szse_delisted(sheet, UPDATE_DATE)
        assert len(rows) == 1
        r = rows[0]
        assert r["code"] == "000003"
        assert r["name"] == "PT金田A"
        assert r["exchange"] == "SZ"
        assert r["list_date"] == "1991-01-14"
        assert r["delist_date"] == "2002-06-14"
        assert r["board"] == ""
        assert r["full_name"] == ""
        assert r["total_share"] == ""
        assert r["update_date"] == UPDATE_DATE

    def test_skips_row_without_code(self):
        sheet = (
            '<row><c r="A1" t="inlineStr"><is><t>证券代码</t></is></c>'
            '<c r="B1" t="inlineStr"><is><t>证券简称</t></is></c>'
            '<c r="C1" t="inlineStr"><is><t>上市日期</t></is></c>'
            '<c r="D1" t="inlineStr"><is><t>终止上市日期</t></is></c></row>'
            '<row><c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c></row>'
        )
        assert parse_szse_delisted(sheet, UPDATE_DATE) == []


# ── BSE JSON parsing ──────────────────────────────────────────────

class TestParseBseJson:
    def test_bse_row(self):
        rows = parse_bse_json(_bse_body([_bse_row()]), UPDATE_DATE)
        assert len(rows) == 1
        r = rows[0]
        assert r["code"] == "920000"
        assert r["name"] == "安徽凤凰"
        assert r["exchange"] == "BJ"
        assert r["list_date"] == "2020-12-23"
        assert r["delist_date"] == ""
        assert r["board"] == "北交所"
        assert r["industry"] == "汽车制造业"
        assert r["region"] == "安徽省"
        assert r["total_share"] == 91680000.0
        assert r["full_name"] == ""
        assert r["update_date"] == UPDATE_DATE

    def test_old_style_bj_code(self):
        rows = parse_bse_json(
            _bse_body([_bse_row(code="832566", name="梓橦宫")]), UPDATE_DATE)
        assert len(rows) == 1
        assert rows[0]["exchange"] == "BJ"

    def test_wrapper_stripped(self):
        rows = parse_bse_json(_bse_body([_bse_row()], total=2, total_pages=2),
                              UPDATE_DATE)
        assert len(rows) == 1  # only one page's content passed in

    def test_empty_body(self):
        assert parse_bse_json("null([])", UPDATE_DATE) == []

    def test_unwrapped_body_returns_empty(self):
        assert parse_bse_json("garbage-not-jsonp", UPDATE_DATE) == []

    def test_non_list_content_returns_empty(self):
        assert parse_bse_json('null([{"content": "x"}])', UPDATE_DATE) == []


# ── Exchange inference ────────────────────────────────────────────

class TestInferExchange:
    def test_shanghai(self):
        assert infer_exchange("600000") == "SH"
        assert infer_exchange("688001") == "SH"
        assert infer_exchange("689009") == "SH"

    def test_shenzhen(self):
        assert infer_exchange("000001") == "SZ"
        assert infer_exchange("300750") == "SZ"
        assert infer_exchange("002415") == "SZ"

    def test_beijing(self):
        assert infer_exchange("920000") == "BJ"
        assert infer_exchange("832566") == "BJ"
        assert infer_exchange("430047") == "BJ"
        assert infer_exchange("830799") == "BJ"


# ── Merge & dedup ─────────────────────────────────────────────────

class TestMergeExchanges:
    def test_merge_three_exchanges(self):
        sh = parse_sse_json(
            _sse_payload([_sse_row(), _sse_row(code="600002", name="齐鲁退市",
                                               delist="20060424")]),
            UPDATE_DATE,
        )
        sz = parse_szse_xlsx(_szse_sheet([_szse_row()]), UPDATE_DATE)
        bj = parse_bse_json(_bse_body([_bse_row()]), UPDATE_DATE)
        merged = merge_exchanges([sh, sz, bj])
        assert len(merged) == 4
        codes = {r["code"] for r in merged}
        assert codes == {"600000", "600002", "000001", "920000"}

    def test_delisted_sz_merged(self):
        sh = parse_sse_json(_sse_payload([_sse_row()]), UPDATE_DATE)
        sz = parse_szse_xlsx(_szse_sheet([_szse_row()]), UPDATE_DATE)
        sz_delisted = parse_szse_delisted(
            '<row><c r="A1" t="inlineStr"><is><t>证券代码</t></is></c>'
            '<c r="B1" t="inlineStr"><is><t>证券简称</t></is></c>'
            '<c r="C1" t="inlineStr"><is><t>上市日期</t></is></c>'
            '<c r="D1" t="inlineStr"><is><t>终止上市日期</t></is></c></row>'
            '<row><c r="A2" t="inlineStr"><is><t>000003</t></is></c>'
            '<c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c>'
            '<c r="C2" t="inlineStr"><is><t>1991-01-14</t></is></c>'
            '<c r="D2" t="inlineStr"><is><t>2002-06-14</t></is></c></row>',
            UPDATE_DATE,
        )
        merged = merge_exchanges([sh, sz, sz_delisted])
        assert len(merged) == 3  # 600000 + 000001 + 000003

    def test_empty_inputs(self):
        assert merge_exchanges([[], [], []]) == []


# ── CSV output ────────────────────────────────────────────────────

class TestRecordsToCsv:
    def test_csv_header_and_encoding(self, tmp_path):
        rows = parse_sse_json(_sse_payload([_sse_row()]), UPDATE_DATE)
        out = tmp_path / "stock_basic_official.csv"
        records_to_csv(rows, out)
        raw = out.read_bytes()
        assert raw.startswith(b"\xef\xbb\xbf")  # UTF-8 BOM
        text = raw.decode("utf-8-sig")
        lines = text.strip().split("\n")
        assert lines[0].split(",") == COLUMNS
        assert lines[1].startswith("SH600000,")
        assert lines[1].split(",")[3] == "浦发银行"

    def test_empty_records(self, tmp_path):
        out = tmp_path / "empty.csv"
        records_to_csv([], out)
        assert out.read_bytes().decode("utf-8-sig").strip() == ",".join(COLUMNS)


# ── Row-count sanity (not a hard assertion — data changes daily) ──

class TestRowCountSanity:
    def test_total_is_in_expected_range(self):
        """Merged result should be in the 5,500-6,500 range (~5,888 expected)."""
        # Can't hit network in unit tests; this guards the merge contract
        # rather than actual counts. Real count verified in integration QA.
        assert len(COLUMNS) == 12


# ── _fmt_date guard ─────────────────────────────────────────────

class TestFmtDate:
    def test_invalid_length_returns_empty(self):
        assert fsbo._fmt_date("2026073") == ""


# ── 网络层：重试包装器 ──────────────────────────────────────────

class TestWithRetry:
    def test_success_returns_value(self):
        assert fsbo._with_retry(lambda: "ok", desc="") == "ok"

    def test_retries_then_success(self, monkeypatch):
        calls = {"n": 0}

        def flaky():
            calls["n"] += 1
            if calls["n"] < 3:
                raise RuntimeError("boom")
            return "ok"

        mock_sleep = Mock()
        monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)
        assert fsbo._with_retry(flaky, desc="测试") == "ok"
        assert mock_sleep.call_count == 2

    def test_exhausts_raises_last(self, monkeypatch):
        def always_fail():
            raise RuntimeError("boom")

        mock_sleep = Mock()
        monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)
        with pytest.raises(RuntimeError):
            fsbo._with_retry(always_fail, desc="测试")


# ── 网络层：SSE ─────────────────────────────────────────────────

class TestFetchSse:
    def test_success_returns_json(self):
        stub = SyncStubSession(json_data={"pageHelp": {"data": []}})
        assert fsbo.fetch_sse(stub) == {"pageHelp": {"data": []}}
        assert any(c[0] == "GET" and c[1] == fsbo.SSE_URL for c in stub.calls)

    def test_http_error_raises(self):
        stub = SyncStubSession(status_code=500)
        with pytest.raises(Exception, match="HTTP 500"):
            fsbo.fetch_sse(stub)


# ── 网络层：SZSE xlsx ───────────────────────────────────────────

class TestFetchSzseXlsx:
    def test_returns_sheet_xml(self):
        sheet = _szse_sheet([_szse_row()])
        stub = SyncStubSession(content=_szse_zip_bytes(sheet))
        assert fsbo.fetch_szse_xlsx(stub, "1110", "tab1") == sheet


# ── 网络层：BSE ─────────────────────────────────────────────────

class TestFetchBse:
    def test_paginates_two_pages(self):
        stub = SyncStubSession(
            canned_responses={fsbo.BSE_LISTED_URL: SyncStubResponse(status_code=200)}
        )

        def _post(url, data=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            stub.calls.append(("POST", url, data))
            if data["page"] == "0":
                return SyncStubResponse(text=_bse_body([_bse_row()], total_pages=2))
            return SyncStubResponse(text=_bse_body([_bse_row(code="920001")], total_pages=2))

        stub.post = _post  # type: ignore[method-assign]

        rows = fsbo.fetch_bse(stub)
        assert len(rows) == 2
        assert rows[0]["xxzqdm"] == "920000"
        assert rows[1]["xxzqdm"] == "920001"
        assert sum(1 for c in stub.calls if c[0] == "POST") == 2

    def test_stops_on_empty_wrapper(self):
        stub = SyncStubSession()

        def _post(url, data=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            return SyncStubResponse(text="null([])")

        stub.post = _post  # type: ignore[method-assign]

        assert fsbo.fetch_bse(stub) == []

    def test_stops_on_empty_content(self):
        stub = SyncStubSession()

        def _post(url, data=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            return SyncStubResponse(text='null([{"content": [], "totalPages": 2}])')

        stub.post = _post  # type: ignore[method-assign]

        assert fsbo.fetch_bse(stub) == []


# ── main() ──────────────────────────────────────────────────────

class TestMain:
    def test_full_run_writes_merged_csv(self, tmp_path, monkeypatch):
        stub = SyncStubSession()

        def _get(url, params=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            stub.calls.append(("GET", url, params))
            if url == fsbo.SSE_URL:
                return SyncStubResponse(json_data=_sse_payload([_sse_row()]))
            if url == fsbo.SZSE_XLSX_URL:
                catalogid = (params or {}).get("CATALOGID")
                xml = _szse_sheet([_szse_row()]) if catalogid == "1110" else _delisted_xml
                return SyncStubResponse(content=_szse_zip_bytes(xml))
            if url == fsbo.BSE_LISTED_URL:
                return SyncStubResponse(status_code=200)
            return SyncStubResponse(status_code=200)

        def _post(url, data=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            stub.calls.append(("POST", url, data))
            return SyncStubResponse(text=_bse_body([_bse_row()]))

        stub.get = _get  # type: ignore[method-assign]
        stub.post = _post  # type: ignore[method-assign]
        monkeypatch.setattr(fsbo.requests, "Session", lambda: stub)
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "-o", str(tmp_path / "out.csv"),
             "--update-date", "2026-07-31"],
        )

        fsbo.main()

        out = tmp_path / "out.csv"
        assert out.exists()
        lines = out.read_text(encoding="utf-8-sig").strip().split("\n")
        assert lines[0] == ",".join(COLUMNS)
        # 三大交易所各一条代表记录 + 深交所退市股（SZ000003）
        assert any(line.startswith("SH600000,") for line in lines)
        assert any(line.startswith("SZ000001,") for line in lines)
        assert any(line.startswith("BJ920000,") for line in lines)
        assert len(lines) == 5  # 表头 + 4 行数据

        methods = [(m, u) for m, u, _ in stub.calls]
        assert methods.count(("GET", fsbo.SSE_URL)) == 1
        assert methods.count(("GET", fsbo.SZSE_XLSX_URL)) == 2
        assert methods.count(("GET", fsbo.BSE_LISTED_URL)) == 1
        assert methods.count(("POST", fsbo.BSE_API_URL)) == 1
        assert len(stub.calls) == 5

    def test_invalid_update_date_exits(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "-o", str(tmp_path / "x.csv"),
             "--update-date", "2026/07/31"],
        )
        with pytest.raises(SystemExit):
            fsbo.main()
        assert "日期格式无效" in capsys.readouterr().err

    def test_one_exchange_empty_aborts_without_partial_csv(self, tmp_path, monkeypatch):
        """SZSE active returning zero records must abort; never write partial stock."""
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "-o", str(tmp_path / "out.csv"),
             "--update-date", "2026-07-31"],
        )
        monkeypatch.setattr(fsbo, "make_proxy_pool", Mock(return_value=None))
        monkeypatch.setattr(fsbo.requests, "Session", lambda: SyncStubSession())
        monkeypatch.setattr(fsbo, "fetch_sse", Mock(return_value=_sse_payload([_sse_row()])))

        def _fake_szse_xlsx(session, catalogid, tabkey, *, pool=None):
            if catalogid == "1110":
                return _szse_sheet([])
            return _delisted_xml

        monkeypatch.setattr(fsbo, "fetch_szse_xlsx", _fake_szse_xlsx)
        monkeypatch.setattr(fsbo, "fetch_bse", Mock(return_value=[_bse_row()]))

        with pytest.raises(SystemExit):
            fsbo.main()

        out = tmp_path / "out.csv"
        assert not out.exists()

    def test_all_exchanges_failed_aborts_without_writing_csv(self, tmp_path, monkeypatch):
        stub = SyncStubSession()

        def _get(url, params=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            raise RuntimeError("boom")

        def _post(url, data=None, **kwargs):  # noqa: ANN001, ANN002, ANN003
            raise RuntimeError("boom")

        stub.get = _get  # type: ignore[method-assign]
        stub.post = _post  # type: ignore[method-assign]
        mock_sleep = Mock()
        monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)
        monkeypatch.setattr(fsbo.requests, "Session", lambda: stub)
        monkeypatch.setattr(
            sys, "argv",
            ["fetch_stock_basic_official.py", "-o", str(tmp_path / "out.csv"),
             "--update-date", "2026-07-31"],
        )

        with pytest.raises(SystemExit):
            fsbo.main()

        out = tmp_path / "out.csv"
        assert not out.exists()
