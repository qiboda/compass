# data-coverage-95-tests — Python collectors 测试计划（83.0% → ≥95%，ref #163）

> Test-agent 产出，供实现 agent 逐条执行。本文件是**决策完备**的：实现者照做即可，
> 不需要重新推导任何 mock 目标、断言或行号。所有行号已对照实际源码核实
> （2026-08-04，`uv run pytest tests/ --cov=. --cov-report=term-missing` 实测输出）。

## 0. 实测基线（不得偏离）

| 文件 | Stmts | Miss | 缺失行 |
|---|---|---|---|
| main.py | 192 | 30 | 260-278, 302-316 |
| fetch_concept_member.py | 134 | 20 | 89-92, 116, 158-161, 179-180, 182-183, 187, 293-299 |
| fetch_fin_indicators.py | 169 | 30 | 79-103, 118-119, 196-197, 201, 205, 278, 298-306, 334-336, 344, 365 |
| fetch_stock_basic_official.py | 227 | 107 | 105, 141-156, 194, 261, 308, 346, 356, 435-441, 455-470, 483-529, 537-624, 628 |
| fetch_stock_basic.py | 121 | 4 | 108-109, 167, 256 |
| fetch_dragon.py | 108 | 15 | 67-72, 253-267 |
| 其余 9 个文件（fetch_stock_basic 4、fetch_dragon 15、fetch_main_flow 15、fetch_balance_sheet 8、fetch_block_trade 9、fetch_cash_flow 8、fetch_income 8、fetch_institution_survey 8、common 7） | — | 82 | 见 §8 回退清单 |
| **TOTAL** | **1583** | **269** | — |

- 现状：256 tests 全过（~108s），TOTAL 83.0%（1314 covered）。
- 4 个低覆盖文件 = 187 stmts，占全部缺失的 70%。
- **覆盖算术（实现者必须理解，否则会误判）**：
  - T5 pragma 共移除 12 行（§3.5）：concept_member 293-299=6、fin_indicators 365=1、
    stock_basic_official 628=1、stock_basic_official 155-156=2、stock_basic 256=1、
    **stock_basic 167=1（本计划新增）** → 报告总 stmts 1583-12 = **1571**。
  - T1-T4 测试覆盖 187-10 = **177** stmts。
  - 仅做 T1-T5 时：1314+177 = 1491，1491/1571 = **94.9% < 95% → FAIL**。
  - **因此 §3.6 的 buffer 测试是强制的**（不是可选项）：
    - test_stock_basic.py Throttle wait 分支（stock_basic 108-109）+2
    - test_dragon.py `_as_float`（fetch_dragon 67-72）+6
  - 最终预期：**1499/1571 = 95.42%**（余量 ~6.6 stmts，足够吸收行数统计误差）。

## 1. 覆盖型工作的 RED 语义（重要）

本任务只加测试、不改生产逻辑——经典 TDD 的"测试先失败"在这里表现为：
**新增测试前，目标行在 `--cov-report=term-missing` 中缺失（RED 证据）；新增测试后，
同一命令下目标行消失（GREEN 证据）**。每个 todo 的 RED 命令 = 用现有测试跑目标文件的
覆盖率，输出必须包含上表列出的缺失行。这是唯一的"失败"判据，pytest 本身会全绿。

## 2. conftest.py — SyncStubSession 设计规格（T4 前置，先做）

追加到 `collectors/tests/conftest.py` 文件末尾（**不动**现有 `StubResponse`/`StubSession`/
`make_stub_session`，0 冲突）。现有 conftest.py 仅 99 行，`StubSession` 是 async + get-only；
fetch_stock_basic_official.py 用的是 **sync `requests.Session`**，且需要 `.content`（bytes，
zip）、`.text`（str，JSONP）、`.headers`（`session.headers.update(...)` at :566）、`.post`。

```python
class SyncStubResponse:
    """Fake requests.Response for sync (non-async) collector call-sites.

    Call-sites use ``resp.status_code`` (int), ``resp.raise_for_status()``
    (sync, raises on >= 400 or injected exception), ``resp.json()`` (canned
    dict), ``resp.content`` (bytes — xlsx zip payloads), ``resp.text`` (str —
    BSE JSONP body).
    """

    __slots__ = ("status_code", "_json", "_content", "_text", "_exc")

    def __init__(
        self,
        *,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        content: bytes = b"",
        text: str = "",
        exc: Exception | None = None,
    ) -> None:
        self.status_code = status_code
        self._json = json_data
        self._content = content
        self._text = text
        self._exc = exc

    def raise_for_status(self) -> None:
        if self._exc is not None:
            raise self._exc
        if self.status_code >= 400:
            raise Exception(f"HTTP {self.status_code}")

    def json(self) -> dict[str, Any]:
        return self._json if self._json is not None else {}

    @property
    def content(self) -> bytes:
        return self._content

    @property
    def text(self) -> str:
        return self._text


class SyncStubSession:
    """Sync stub for ``requests.Session`` — .get/.post return SyncStubResponse.

    Same injection API as StubSession: ``canned_responses`` keyed by URL
    (values are SyncStubResponse or kwargs dicts), per-test closure override
    (``stub.get = _get`` / ``stub.post = _post``). ``headers`` is a plain
    dict so ``session.headers.update(...)`` works; ``calls`` logs every
    (method, url, params/data) for assertion.
    """

    def __init__(
        self,
        *,
        canned_responses: dict[str, SyncStubResponse | dict[str, Any]] | None = None,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        content: bytes = b"",
        text: str = "",
        exc: Exception | None = None,
    ) -> None:
        self._canned = canned_responses or {}
        self._status_code = status_code
        self._json_data = json_data
        self._content = content
        self._text = text
        self._exc = exc
        self.headers: dict[str, Any] = {}
        self.calls: list[tuple[str, str, Any]] = []  # (method, url, params/data)

    def _dispatch(self, method: str, url: str, params: Any) -> SyncStubResponse:
        self.calls.append((method, url, params))
        cfg = self._canned.get(url)
        if cfg is not None:
            if isinstance(cfg, SyncStubResponse):
                return cfg
            return SyncStubResponse(**cfg)  # type: ignore[arg-type]
        return SyncStubResponse(
            status_code=self._status_code,
            json_data=self._json_data,
            content=self._content,
            text=self._text,
            exc=self._exc,
        )

    def get(self, url: str, params: Any = None, headers: Any = None,
            timeout: Any = None) -> SyncStubResponse:
        return self._dispatch("GET", url, params)

    def post(self, url: str, data: Any = None, headers: Any = None,
             timeout: Any = None) -> SyncStubResponse:
        return self._dispatch("POST", url, data)
```

设计要点（实现者照做）：
- **canned API 决策**：URL → 固定响应 用 `canned_responses`；需要按 `params`/`data`
  分派的（SZSE 两个 CATALOGID、BSE 分页）用闭包覆盖 `stub.get = _get` /
  `stub.post = _post` —— 与 test_concept_member.py:264-274、test_fin_indicators.py:110-122
  的既有惯例完全一致。
- **fetch_szse_xlsx 的 zip**：canned 响应用 `content=` 传真实 zip 字节（测试内用
  `zipfile.ZipFile(BytesIO(...), "w").writestr("xl/worksheets/sheet1.xml", xml)` 构造，
  见 §3.4 helper）。
- **fetch_bse 分页**：`stub.post = _post` 闭包读 `data["page"]`，逐页返回
  `SyncStubResponse(text='null([{"content": [...], "totalPages": N}])')`。
- 不 import requests、不抛 requests.HTTPError（与现有 StubResponse 的
  `Exception(f"HTTP {status_code}")` 保持一致；生产端 `_with_retry` 只 catch Exception）。

## 3. 逐文件测试用例清单（按 7 个 plan todos 分组）

### 3.1 T1 — main.py（30 stmts：260-278, 302-316）→ `collectors/tests/test_main.py`

**方式**：完全复制现有 Mock 模式（test_main.py:117-131 的
`test_balance_sheet_calls_run_via_asyncio` 与 :205-215 的
`test_balance_sheet_calls_import_to_dolt`），向 `TestDispatchFetch` /
`TestDispatchImport` 类**末尾追加**方法，不动已有代码。

| # | 测试方法（类名已存在，仅加方法） | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 1.1 | `TestDispatchFetch::test_dragon_calls_run_via_asyncio` | `import fetch_dragon as fdr; import main as main_mod`；`mock_run = Mock(); monkeypatch.setattr(main_mod.asyncio, "run", mock_run)`；`mock_fdr_run = Mock(); monkeypatch.setattr(fdr, "run", mock_fdr_run)`；`main_mod.dispatch_fetch("dragon")` | `mock_run.assert_called_once()` | 260-262 |
| 1.2 | `test_block_trade_calls_run_via_asyncio` | 同上，`fetch_block_trade` + `.run` | `mock_run.assert_called_once()` | 264-266 |
| 1.3 | `test_institution_survey_calls_run_via_asyncio` | 同上，`fetch_institution_survey` + `.run` | `mock_run.assert_called_once()` | 268-270 |
| 1.4 | `test_concept_member_calls_run_via_asyncio` | 同上，`fetch_concept_member` + `.run` | `mock_run.assert_called_once()` | 272-274 |
| 1.5 | `test_main_flow_calls_run_via_asyncio` | 同上，`fetch_main_flow` + `.run` | `mock_run.assert_called_once()` | 276-278 |
| 1.6 | `TestDispatchImport::test_dragon_calls_import_to_dolt` | `monkeypatch.setattr(fdr, "import_to_dolt", mock_import)`；`main_mod.dispatch_import("dragon")` | `mock_import.assert_called_once()` | 302-304 |
| 1.7 | `test_block_trade_calls_import_to_dolt` | 同上，`fetch_block_trade` | `mock_import.assert_called_once()` | 305-307 |
| 1.8 | `test_institution_survey_calls_import_to_dolt` | 同上，`fetch_institution_survey` | `mock_import.assert_called_once()` | 308-310 |
| 1.9 | `test_concept_member_calls_import_to_dolt` | 同上，`fetch_concept_member` | `mock_import.assert_called_once()` | 311-313 |
| 1.10 | `test_main_flow_calls_import_to_dolt` | 同上，`fetch_main_flow` | `mock_import.assert_called_once()` | 314-316 |

注意：**不要**为这 5 个 target 重复已有的 stock_basic/fin_indicators/balance_sheet/
income/cash_flow 测试；`do_sync` 测试（TestDoSync）已存在且已覆盖 sync 全链路。

### 3.2 T2 — fetch_concept_member.py（20 stmts）→ `collectors/tests/test_concept_member.py`

**方式**：现有 URL 分派闭包模式（test_concept_member.py:296-303 `_make_get`；
429 参考 test_fin_indicators.py:102-130）。新增两个类，**追加到文件末尾**。
复用模块级 helper `_board_list_json`（:48-56）与 `_member_json`（:59-64）；
`from common import Throttle` 或直接 `fetch_concept_member.Throttle`（模块已 import）。
所有测试 `monkeypatch.setattr(asyncio, "sleep", mock_sleep)`（AsyncMock），
`t = Throttle(min_interval=0)`。

**fetch_board_list（:60-118）** — 新类 `TestFetchBoardList`：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 2.1 | `test_429_retries_then_success` | 闭包 `_get`：第 1 次调用返回 `StubResponse(status_code=429)`；第 2 次返回 `_board_list_json([("BK1169", "Kimi概念")])`。`await fetch_board_list(stub, t)` | `boards == [("BK1169", "Kimi概念")]` | 88-92（429 wait/print/sleep/continue），93-95，107-115，118 |
| 2.2 | `test_paginates_multiple_pages` | 闭包按 `params["pn"]` 分派；**注意 `_board_list_json` 的 total=len(boards)，分页必须手工构造**：`{"rc": 0, "data": {"total": 250, "diff": [{"f12": c, "f14": n} for c, n in page]}}`；pn=1/2 各 100 条，pn=3 返回 50 条 | `len(boards) == 250`（页1: 100<250 → 116 page+=1；页2: 200<250 → 再进；页3: 250>=250 → 114 break） | 116（page += 1），114 的 False 分支 |
| 2.3 | `test_429_always_leaves_no_data` | 闭包恒返 `StubResponse(status_code=429)` | `boards == []`（4 次尝试后循环自然结束 → 107 diff=[] → 114 `not diff` → break） | 88-92 全路径 + 107-115 空数据路径 |

**fetch_board_members（:121-199）** — 新类 `TestFetchBoardMembers`（stub 闭包与
:296-301 相同：`flt = (params or {}).get("filter", ""); code = flt.split('"')[1] ...`）：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 2.4 | `test_429_retries_then_success` | 闭包：第 1 次 429，第 2 次 `_member_json([{"SECUCODE": "600880.SH", "SECURITY_CODE": "600880", "NEW_BOARD_CODE": "BK1169", "BOARD_NAME": "Kimi概念"}])`。`await fetch_board_members(stub, t, "BK1169", 100)` | `len(records) == 1` | 157-165（429 + success），189-197（items/extend/total_pages/page+=1） |
| 2.5 | `test_429_always_leaves_data_none` | 闭包恒返 429 | `records == []`（4 次后 data=None → 178-180 "No data returned" + break） | 157-161 + **179-180** |
| 2.6 | `test_success_false_breaks` | 返回 `{"success": False, "message": "API down"}` | `records == []` | **182-183** |
| 2.7 | `test_result_none_breaks` | 返回 `{"success": True, "result": None}` | `records == []` | **185-187**（187 = result None 的 break） |

（fetch_board_members 的 196-197 多页已被现有 run() 测试覆盖，无需新增；
`__main__` 293-299 留给 T5 pragma。）

### 3.3 T3 — fetch_fin_indicators.py（30 stmts）→ `collectors/tests/test_fin_indicators.py`

**方式**：向现有类追加方法 + 新增 `TestThrottle` 类。注意本文件当前**没有**
`from datetime import datetime`（T3.8 需新增该 import）。子进程测试用真实 temp dolt
（模式照抄 test_import_to_dolt.py:92-128 的 `dolt init` + `dolt --data-dir ... sql`）。

**TestLastReportDate（:157-199 已有 3 个 fallback 测试，追加）**：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 3.1 | `test_dolt_subprocess_returns_max_report_date` | ① `tmp_path` 上 `dolt init`（照抄 test_import_to_dolt.py:98-110 的 config+init，断言 returncode==0）；② `dolt sql` 建 `fin_indicators (symbol VARCHAR(20) PRIMARY KEY, report_date DATE NOT NULL)` 并 INSERT `('SZ000001', '2024-12-31')`；③ 重定向模块的 `Path.__truediv__`：`real_truediv = Path.__truediv__; def _redirect(self, other): return tmp_path if other == "compass_data" else real_truediv(self, other); monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)`（使 :73 的 `... / "compass_data"` 落到 tmp_path，`.dolt` 由 dolt init 创建）；④ 调 `_last_report_date("RPT_LICO_FN_CPD", tmp_path / "nonexistent.state.json")` | `== "2024-12-31"` | 79（table 映射），80-99（subprocess 成功路径：returncode==0 → last 非空非 NULL → return） |
| 3.2 | `test_dolt_subprocess_null_falls_back_to_state` | 同上但 fin_indicators 表**为空**（MAX 为 NULL，或 dolt 输出空行——两种输出都会走 fallback）；state 文件写 `json.dumps({"last_report_date": "2024-12-31"})` | `== "2024-12-31"` | 95-98 False 分支，101-102（state 回退） |

（可选 3.3：不建表直接查 → returncode != 0 → 同样回退 state。3.2 已覆盖 101-102，
3.3 仅为稳妥，可跳过。）

**新类 `TestThrottle`（fetch_fin_indicators.py:109-122）**：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 3.4 | `test_acquire_waits_when_below_min_interval` | `mock_sleep = AsyncMock(); monkeypatch.setattr(asyncio, "sleep", mock_sleep)`；`t = fetch_fin_indicators.Throttle(min_interval=10)`；`await t.acquire()`（首次：since_last 巨大 → 121 else 分支，已有覆盖）；紧接着 `await t.acquire()`（since_last≈0 < 10 → wait 分支） | `mock_sleep.call_count >= 2` | 117-119（118 wait 计算 + 119 asyncio.sleep） |

**TestFetchPeriod（:79-152 已有 3 个测试，追加）**——全部 `make_stub_session(json_data=...)` + `Throttle(min_interval=0)`：

| # | 测试方法 | 返回体 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 3.5 | `test_success_false_breaks` | `{"success": False, "message": "boom"}` | `records == []` | 195-197 |
| 3.6 | `test_result_none_breaks` | `{"success": True, "result": None}` | `records == []` | 199-201 |
| 3.7 | `test_empty_items_breaks` | `{"success": True, "result": {"data": [], "pages": 1}}` | `records == []` | 203-205 |

**TestMain（:205-240 已有 1 个测试，追加）**——全部 `patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub)` + `monkeypatch.chdir(tmp_path)` + `mock_sleep`：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 3.8 | `test_default_years_covers_2020_to_now` | **不传 `--years`**；`monkeypatch.setattr(fetch_fin_indicators, "datetime", FakeDatetime)`，其中 `class FakeDatetime(datetime): @classmethod def now(cls, tz=None): return cls(2020, 6, 1, 12, 0, 0)`（需在测试文件头部 `from datetime import datetime`；FakeDatetime 是子类，`:352 datetime.now().isoformat()` 也能工作）。sys.argv = `["fetch_fin_indicators.py", "--periods", "FY"]`；stub json 同现有 test_main_with_report_name（success/data 1 条/pages 1） | csv 存在；state `last_report_date == "2020-12-31"`（years=[2020] → 1 个日期） | **278**（默认 years），269-270，275 的 False 分支 |
| 3.9 | `test_incremental_filters_dates` | sys.argv = `["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"]`；`monkeypatch.setattr(fetch_fin_indicators, "_last_report_date", lambda *a, **k: "2023-01-01")`；stub 正常 json | csv 存在（2024-12-31 ≥ since → 继续抓取） | 297-301（if incremental / since= / if since / print / list 过滤） |
| 3.10 | `test_incremental_no_new_periods_returns` | 同上但 `_last_report_date` 返回 `"2025-01-01"`（全部日期被过滤） | **csv 不存在**、**state 不存在**（:306 return 提前退出） | 298-301，304-306（"No new report periods to fetch." + return） |
| 3.11 | `test_incremental_no_prior_data_full_fetch` | 同上但 `_last_report_date` 返回 `""` | csv 存在（:303 打印 "No prior data found, fetching full history." 后全量抓取） | 302-303 |
| 3.12 | `test_period_fetch_failure_continues` | `make_stub_session(exc=RuntimeError("boom"))`（每次 get 都抛 → fetch_period 重试 4 次后 raise → main catch） | csv 不存在、state 不存在；打印包含 "FAILED: boom" | 332-336（try/except/print/continue） |
| 3.13 | `test_empty_records_prints_empty` | stub json = `{"success": True, "result": {"data": [], "pages": 1}}` | csv 不存在、state 不存在（`if records:` False → else "empty"） | 343-344 |

### 3.4 T4 — fetch_stock_basic_official.py（107 stmts）→ `collectors/tests/test_stock_basic_official.py` + conftest

**文件头新增**：`import fetch_stock_basic_official as fsbo`（现有 :27-36 是 from-import，
新增模块级别名，两者共存）。新增 helper：

```python
import io
import zipfile

def _szse_zip_bytes(sheet_xml: str) -> bytes:
    """Build a real xlsx zip whose sheet1.xml is sheet_xml (fetch_szse_xlsx unzips it)."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("xl/worksheets/sheet1.xml", sheet_xml)
    return buf.getvalue()
```

**解析器 guard（纯函数，追加到现有类）**：

| # | 测试方法（类） | 输入 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 4.1 | `TestFmtDate::test_invalid_length_returns_empty`（新类） | `fsbo._fmt_date("2026073")` | `== ""` | **105**（len != 8 → return ""） |
| 4.2 | `TestParseSseJson::test_non_list_data_returns_empty` | `parse_sse_json({"pageHelp": {"data": {}}}, UPDATE_DATE)` | `== []` | **193-194**（非 list → return records） |
| 4.3 | `TestParseSzseXlsx::test_skips_row_without_code` | `_szse_sheet(['<row><c r="A1" t="inlineStr"><is><t>主板</t></is></c><c r="F1" t="inlineStr"><is><t>无代码</t></is></c></row>'])` | `== []`（该行无 E 单元格 → code="" → continue） | **259-261** |
| 4.4 | `TestParseSzseDelisted::test_skips_row_without_code` | 表头 + `'<row><c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c></row>'` | `== []`（无 A 单元格） | **306-308** |
| 4.5 | `TestParseBseJson::test_unwrapped_body_returns_empty` | `parse_bse_json("garbage-not-jsonp", UPDATE_DATE)` | `== []` | **345-346**（非 null(...) 包裹） |
| 4.6 | `TestParseBseJson::test_non_list_content_returns_empty` | `parse_bse_json('null([{"content": "x"}])', UPDATE_DATE)` | `== []` | **355-356**（content 非 list） |

**网络层（新类，全部用 SyncStubSession）**：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 4.7 | `TestWithRetry::test_success_returns_value`（新类） | `fsbo._with_retry(lambda: "ok", desc="")` | `== "ok"` | 141-144（循环 + try + return） |
| 4.8 | `test_retries_then_success` | fn 前 2 次 raise 第 3 次返回；`monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)`（Mock，防真实 sleep 1+2=3s） | `== "ok"`；`mock_sleep.call_count == 2` | 145-151（except/last_exc/if attempt<max/wait/print/sleep） |
| 4.9 | `test_exhausts_raises_last` | fn 恒 raise | `pytest.raises(RuntimeError)` | 145-147，152-153（else → raise） |
| 4.10 | `TestFetchSse::test_success_returns_json`（新类） | `stub = SyncStubSession(json_data={"pageHelp": {"data": []}})`；`fsbo.fetch_sse(stub)` | 返回 dict 与 canned 相同；`("GET", fsbo.SSE_URL, _) in stub.calls`（可用 `any(c[0] == "GET" and c[1] == fsbo.SSE_URL for c in stub.calls)`） | 435-441（headers/get/raise_for_status/json） |
| 4.11 | `test_http_error_raises` | `SyncStubSession(status_code=500)` | `pytest.raises(Exception)`（raise_for_status 路径） | 440 的异常路径 |
| 4.12 | `TestFetchSzseXlsx::test_returns_sheet_xml`（新类） | `stub = SyncStubSession(content=_szse_zip_bytes(_szse_sheet([_szse_row()])))`；`fsbo.fetch_szse_xlsx(stub, "1110", "tab1")` | 返回字符串 == `_szse_sheet([_szse_row()])` | 455-470（params/headers/get/zip 解压/read/decode） |
| 4.13 | `TestFetchBse::test_paginates_two_pages`（新类） | `stub.get` 对 `fsbo.BSE_LISTED_URL` 返回 `SyncStubResponse(status_code=200)`（可用 canned）；`stub.post = _post` 闭包：`data["page"] == "0"` → `SyncStubResponse(text='null([{"content": [r1], "totalPages": 2}])')`；`"1"` → 同结构 r2（r1/r2 用现有 `_bse_row()` 字符串，r2 改 code="920001"）。`fsbo.fetch_bse(stub)` | `len(rows) == 2`；`stub.calls` 含 POST 两次 | 483-487（GET 列表页），489-490，492-493，501，507-509（post/raise/body），512-513，517-518，522，524-527（total_pages/page+=1/if page>=total_pages break），529 |
| 4.14 | `test_stops_on_empty_wrapper` | `stub.post` 返回 `SyncStubResponse(text="null([])")` | `rows == []` | 514-515（wrapper 空 → break） |
| 4.15 | `test_stops_on_empty_content` | `stub.post` 返回 `SyncStubResponse(text='null([{"content": [], "totalPages": 2}])')` | `rows == []` | 519-520（content 空 → break） |

**main()（新类 `TestMain`）**——`monkeypatch.setattr(fsbo.requests, "Session", lambda: stub)`（main :565 的 `requests.Session()` 被 stub 替换；:566 `session.headers.update` 由 `stub.headers` 承载）+ `monkeypatch.setattr(sys, "argv", ...)` + 失败测试里 `monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)`：

| # | 测试方法 | Mock 方式 | 断言 | 覆盖行 |
|---|---|---|---|---|
| 4.16 | `test_full_run_writes_merged_csv` | 闭包 `_get(url, params=None, ...)`：`url == fsbo.SSE_URL` → `SyncStubResponse(json_data=_sse_payload([_sse_row()]))`；`url == fsbo.SZSE_XLSX_URL` → `SyncStubResponse(content=_szse_zip_bytes(_szse_sheet([_szse_row()]) if (params or {}).get("CATALOGID") == "1110" else _delisted_xml))`（_delisted_xml = 现有 TestParseSzseDelisted.test_delisted_row 内联 sheet，:246-253，提为模块常量）；`url == fsbo.BSE_LISTED_URL` → `SyncStubResponse(status_code=200)`；`_post` → `SyncStubResponse(text=_bse_body([_bse_row()]))`。argv = `["fetch_stock_basic_official.py", "-o", str(tmp_path / "out.csv"), "--update-date", "2026-07-31"]`。`fsbo.main()` | csv 存在且 3 行数据（SH600000/SZ000001/BJ920000）；`stub.calls` 含 5 个端点（SSE GET、SZSE GET×2、BSE_LISTED GET、BSE_API POST） | 537-566（argparse/日期校验通过/session），572-577（SSE try），582-589（SZSE 正常），594-601（SZSE 退市），606-613（BSE），618-624（merge/去重/写 CSV/完成） |
| 4.17 | `test_invalid_update_date_exits` | argv = `["fetch_stock_basic_official.py", "-o", str(tmp_path / "x.csv"), "--update-date", "2026/07/31"]`；不需要 stub（:557 strptime 抛 ValueError 在 session 创建前） | `pytest.raises(SystemExit)`；stderr 含 "日期格式无效" | 556-560 |
| 4.18 | `test_all_exchanges_failed_still_writes_header` | `_get`/`_post` 恒 `raise RuntimeError("boom")`；`monkeypatch.setattr(fsbo.time, "sleep", mock_sleep)`（3 次重试 sleep 1+2=3s，必须 mock）；argv 同 4.16 | csv 存在且仅表头（1 行 = COLUMNS join） | 573-579（SSE except），583-591（SZSE except），595-603（退市 except），607-615（BSE except），618-624（空 merge → 仅表头） |

### 3.5 T5 — `# pragma: no cover`（12 行，含理由注释；先例 main.py:448）

逐字插入（只加行尾注释，不改逻辑）：

| 文件:行 | 现状 | 插入后 |
|---|---|---|
| fetch_stock_basic_official.py:627 | `if __name__ == "__main__":` | `if __name__ == "__main__":  # pragma: no cover — __main__ block, never executed under pytest`（整块 628 一并排除，同 main.py:448 先例） |
| fetch_fin_indicators.py:364 | `if __name__ == "__main__":` | 同上 |
| fetch_concept_member.py:292 | `if __name__ == "__main__":` | 同上（293-299 整块排除） |
| fetch_stock_basic.py:255 | `if __name__ == "__main__":` | 同上 |
| fetch_stock_basic_official.py:155 | `assert last_exc is not None` | `assert last_exc is not None  # pragma: no cover — unreachable mypy-required code (loop always returns or raises)` |
| fetch_stock_basic_official.py:156 | `raise last_exc` | `raise last_exc  # pragma: no cover — unreachable mypy-required code (loop always returns or raises)` |
| fetch_stock_basic.py:167 | `return []` | `return []  # pragma: no cover — unreachable (loop always returns or raises)`（for 循环内每轮必 return 或 raise，循环后不可达；mypy 需要该语句） |

### 3.6 Buffer（强制，见 §0 算术）— 2 个小测试文件

**B1 — test_stock_basic.py（fetch_stock_basic.py:108-109，2 stmts）**：新类 `TestThrottle`
追加到文件末尾。`from unittest.mock import AsyncMock` 已存在（:6）；`Throttle` 已 import（:11）。
`mock_sleep = AsyncMock(); monkeypatch.setattr(asyncio, "sleep", mock_sleep); t = Throttle(min_interval=10); await t.acquire(); await t.acquire(); assert mock_sleep.call_count >= 2`
→ 覆盖 117 False（首次，已有）+ **117 True → 108-109**。测试名：`test_acquire_waits_when_below_min_interval`。

**B2 — test_dragon.py（fetch_dragon.py:67-72，6 stmts）**：新类 `TestAsFloat` 追加到文件
末尾，import 行（现有 `from fetch_dragon import ...`）加入 `_as_float`：

| 测试方法 | 输入 | 断言 | 覆盖行 |
|---|---|---|---|
| `test_string_parsed` | `_as_float("123.45")` | `== 123.45` | 67-69（str 且非空 + float() 成功） |
| `test_string_invalid_returns_zero` | `_as_float("abc")` | `== 0.0` | 67-71（ValueError → pass → 72 return 0.0） |
| `test_none_returns_zero` | `_as_float(None)` | `== 0.0` | 72 |

## 4. 实现顺序（最小化 merge 冲突）

**文件间顺序**：
1. `conftest.py`（T4 前置，仅追加类，独立文件 0 冲突）→ 可与其他 todo 并行。
2. Wave 1 并行：T1（test_main.py）、T2（test_concept_member.py）、T3（test_fin_indicators.py
   + test_stock_basic.py）、T4（test_stock_basic_official.py）。
3. Buffer B2（test_dragon.py）与 Wave 1 并行（独立文件）。
4. T5 pragma（5 个生产文件；必须在 T1-T4 之后，避免与测试 commit 抢同一文件的行号上下文）。

**文件内顺序（每个测试文件）**：
- `test_main.py`：只向两个现有类尾部追加方法；不改任何既有行。
- `test_concept_member.py`：模块级 helper 不动；两个新类 `TestFetchBoardList`、
  `TestFetchBoardMembers` 追加到文件末尾（TestRun 之后）。
- `test_fin_indicators.py`：头部补 `from datetime import datetime`；向
  TestLastReportDate / TestFetchPeriod / TestMain 追加方法（类体末尾，下个类定义前）；
  新类 `TestThrottle` 放 TestFetchPeriod 之后。
- `test_stock_basic_official.py`：① 头部加 `import fetch_stock_basic_official as fsbo` +
  `import io, zipfile` + `_szse_zip_bytes` + `_delisted_xml` 常量；② 解析器 guard 追加到
  现有类；③ 新类 TestFmtDate/TestWithRetry/TestFetchSse/TestFetchSzseXlsx/TestFetchBse/
  TestMain 依次追加在文件末尾。
- `test_stock_basic.py` / `test_dragon.py`：新类追加到文件末尾。
- 所有追加均为纯 append / 类内末尾插入 → 与既有代码零重叠，rebase/合并无冲突。

## 5. RED/GREEN 验证命令（逐 todo）

> 所有命令在 `collectors/` 目录下执行。RED 判据见 §1：目标行在 term-missing 中缺失。
> `-q` 覆盖 pyproject 的 `addopts = ["-v"]`。

| Todo | RED（写测试前） | GREEN（写测试后） |
|---|---|---|
| T1 | `uv run pytest tests/test_main.py -q --cov=main --cov-report=term-missing` → Missing 含 **260-278, 302-316** | 同一命令 → main.py Missing 为空（100%） |
| T2 | `uv run pytest tests/test_concept_member.py -q --cov=fetch_concept_member --cov-report=term-missing` → Missing 含 **89-92, 116, 158-161, 179-180, 182-183, 187** | 同一命令 → Missing 仅剩 293-299（T5 后消失） |
| T3+B1 | `uv run pytest tests/test_fin_indicators.py tests/test_stock_basic.py -q --cov=fetch_fin_indicators --cov=fetch_stock_basic --cov-report=term-missing` → Missing 含 **79-103, 118-119, 196-197, 201, 205, 278, 298-306, 334-336, 344, 365** 与 **108-109, 167, 256** | 同一命令 → fin 仅剩 365；stock_basic 仅剩 167, 256（T5 后消失） |
| T4 | `uv run pytest tests/test_stock_basic_official.py -q --cov=fetch_stock_basic_official --cov-report=term-missing` → Missing 含 **105, 141-156, 194, 261, 308, 346, 356, 435-441, 455-470, 483-529, 537-624, 628** | 同一命令 → Missing 仅剩 155-156, 628（T5 后消失） |
| B2 | `uv run pytest tests/test_dragon.py -q --cov=fetch_dragon --cov-report=term-missing` → Missing 含 **67-72** | 同一命令 → 67-72 消失 |
| T5 | `grep -n "pragma: no cover" *.py` → 仅 main.py:448 一处 | 同一 grep → 8 处（main 448 + 本计划 7 处）；再跑一次 T2/T3/T4 的 GREEN 命令确认对应行消失 |
| 最终 gate | — | `uv run pytest tests/ --cov=. --cov-fail-under=95 --cov-report=term-missing` → **退出码 0**，TOTAL ≥ 95%（预期 95.42%，1571 stmts / 1499 covered / 72 missing） |

附加质量检查（每次 commit 前）：
- `uv run ruff check tests/`（pyproject select 含 E/F/I/N/W/UP/B/SIM/C4，line-length 100；
  test 文件现有模式已合规，新代码照抄即可）
- `uv run mypy tests/` 若 CI 配置了（pyproject strict=true；现有测试用 `# type: ignore` 处
  照抄其风格——例如 `stub.get = _get  # type: ignore[method-assign]`）

每个 todo 的 pytest 输出存入 `.omo/evidence/task-<N>-data-coverage-95.txt`（需求计划
约定的证据路径；B1/B2 归入 task-3 / task-2 的 evidence 文件即可，或单独 task-4 附注）。

## 6. Commit 计划（全部 ref #163）

| Commit | 内容 | message |
|---|---|---|
| 1 | test_main.py（T1，10 个方法） | `test(collectors): cover main.py dispatch tail branches for 5 targets` |
| 2 | test_concept_member.py（T2）+ test_dragon.py（B2） | `test(collectors): cover concept_member 429, pagination and guard branches` |
| 3 | test_fin_indicators.py（T3）+ test_stock_basic.py（B1） | `test(collectors): cover fin_indicators incremental, retry and guard branches` |
| 4 | conftest.py + test_stock_basic_official.py（T4） | `test(collectors): cover stock_basic_official network layer with sync stubs` |
| 5 | 5 个生产文件 pragma（T5） | `test(collectors): mark __main__ blocks and unreachable code no-cover` |

commit 顺序 = 上表 1→5（测试先于 pragma，行号上下文稳定）。之后 T6（ci）、T7（docs）
按需求计划执行。B1/B2 不建独立 commit，归入 T3/T2 的主题 commit（同 ref #163）。

## 7. 验收标准（实现完成后逐条核对）

- [ ] `cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95 --cov-report=term-missing` 退出 0，TOTAL ≥ 95%
- [ ] term-missing 中 4 个目标文件 + fetch_stock_basic.py + fetch_dragon.py 缺失行均为 0
- [ ] `grep -n "pragma: no cover" collectors/*.py` 8 处且全部带理由注释
- [ ] `uv run ruff check tests/` 无告警；256 个既有测试未被修改/删除（`git diff` 核对测试文件只增不改）
- [ ] 未改动任何生产代码逻辑（T5 仅行尾注释）；未 mock 整层网络（fetch_stock_basic_official
      全部走 SyncStubSession 真实函数调用）

## 8. 回退清单（仅在最终 gate < 95% 时启用，先做 §0 算术核对再动）

| 文件:行 | 性质 | 方式 | 预计 |
|---|---|---|---|
| common.py:27 | 模块导入期 `logging.basicConfig`（pytest logging 插件先装了 root handlers 故测试中不可达） | `# pragma: no cover — import-time logging bootstrap; pytest logging plugin installs root handlers first`（需用户认可"测试环境不可达"理由，慎用） | +1（分母） |
| fetch_main_flow.py:103/132/135/168-171/206/273-281 | retry/guard/main 循环分支 | 沿用 test_main_flow.py 现有 StubSession 闭包模式（与 T2 同构） | +15 |
| fetch_balance_sheet.py:212-224、fetch_income.py:187-199、fetch_cash_flow.py:190-202、fetch_institution_survey.py:164-176 | 各自 main 循环错误/空数据路径 | 照抄 T3.12/T3.13 模式（stub exc= / data=[]）到对应 test_X.py | 各 +6~8 |
| common.py:136-137/149/166/358-359 | import_replace_table / fetch_paginated 分支 | 429-always stub（common.fetch_paginated 的 358-359 与 T2.5 同构） | +5 |

若启用回退，同样走 RED（先跑 `--cov=<file> --cov-report=term-missing` 确认行缺失）→
GREEN → 归入对应 commit（同 ref #163）。
