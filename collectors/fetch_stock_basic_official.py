#!/usr/bin/env python3
"""从三大交易所官方网站爬取A股股票基本信息并输出CSV。

替代被东方财富污染的数据源（#78）。直接从沪、深、北交易所官方API获取，
避免第三方聚合带来的质量与合规风险。

数据覆盖：上海A股（主板/科创板，不含B股）、深圳A股（主板/创业板）+ 退市股、
北京北交所。各交易所字段不同；统一到 12 列标准 schema 后合并输出。

Usage:
    uv run collectors/fetch_stock_basic_official.py                  # 输出 stock_basic_official.csv
    uv run collectors/fetch_stock_basic_official.py -o stocks.csv    # 自定义输出路径
    uv run collectors/fetch_stock_basic_official.py --update-date 2026-07-31
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import sys
import time
import zipfile
from datetime import date, datetime
from io import BytesIO
from pathlib import Path
from typing import Any

import requests

# ── Constants ──────────────────────────────────────────────────────────────

# Dolt stock_basic 表最终 schema（12 列）
COLUMNS = [
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

# 浏览器 UA（三大交易所均需）
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
)

# ── API endpoints ─────────────────────────────────────────────────────────

# 上交所 — JSON query (单次拉取全量，最大 3000 行/页，当前~2300 行)
SSE_URL = "https://query.sse.com.cn/sseQuery/commonQuery.do"
SSE_PARAMS = {
    "sqlId": "COMMON_SSE_CP_GPJCTPZ_GPLB_GP_L",
    "type": "inParams",
    "STYPE": "3",
    "isPagination": "true",
    "pageHelp.pageSize": "3000",
    "pageHelp.pageNo": "1",
}

# 深交所 — xlsx 报表（CATALOGID=1110 正常上市，1793_ssgs 退市）
SZSE_XLSX_URL = "https://www.szse.cn/api/report/ShowReport"

# 北交所 — JSONP 接口（null(...) wrapper，需分页）
BSE_LISTED_URL = "https://www.bse.cn/nq/listedcompany.html"
BSE_API_URL = "https://www.bse.cn/nqxxController/nqxxCnzq.do"

# 重试
MAX_RETRIES = 3

# ── Regex patterns — XLSX sheet1.xml 解析 ─────────────────────────────────

# 匹配 <row>...</row> 元素（真实 xlsx 带 r="N" 属性）
_ROW_RE = re.compile(r"<row[^>]*>(.*?)</row>", re.DOTALL)

# 匹配 inlineStr 单元格：<c r="A1" s="1" t="inlineStr"><is><t>内容</t></is></c>
_INLINE_STR_RE = re.compile(
    r'<c\s+r="([A-Z]+)\d+"[^>]*t="inlineStr"[^>]*>\s*<is><t[^>]*>(.*?)</t></is>\s*</c>',
    re.DOTALL,
)

# 匹配数字/共享字符串单元格：<c r="H1" s="1"><v>12345</v></c>
_NUMBER_CELL_RE = re.compile(
    r'<c\s+r="([A-Z]+)\d+"[^>]*>\s*<v>([^<]*)</v>\s*</c>',
    re.DOTALL,
)


# ── 辅助函数 ──────────────────────────────────────────────────────────────


def _fmt_date(yyyymmdd: str) -> str:
    """YYYYMMDD → YYYY-MM-DD，无效输入返回空字符串。"""
    s = yyyymmdd.strip()
    if len(s) == 8:
        return f"{s[:4]}-{s[4:6]}-{s[6:8]}"
    return ""


def _parse_xlsx_cells(row_xml: str) -> dict[str, str]:
    """从单个 <row> 元素中提取单元格值，以列字母为键（A, B, ...）。

    覆盖两种单元格类型：inlineStr（t="inlineStr"）和数字（<v>）。
    数字单元格仅在 inlineStr 未覆盖该列时使用。
    """
    cells: dict[str, str] = {}

    for m in _INLINE_STR_RE.finditer(row_xml):
        cells[m.group(1)] = m.group(2)

    for m in _NUMBER_CELL_RE.finditer(row_xml):
        col = m.group(1)
        if col not in cells:  # inlineStr 优先
            cells[col] = m.group(2)

    return cells


def _with_retry(fn, *args: Any, desc: str = "", **kwargs: Any) -> Any:
    """带指数退避重试的网络调用包装器。

    Args:
        fn: 要执行的函数
        desc: 任务描述（用于日志）
        *args, **kwargs: 传递给 fn 的参数

    Returns:
        fn 的返回值

    Raises:
        最后一次尝试的异常（3 次全部失败时）
    """
    last_exc: Exception | None = None
    for attempt in range(MAX_RETRIES):
        try:
            return fn(*args, **kwargs)
        except Exception as exc:
            last_exc = exc
            if attempt < MAX_RETRIES - 1:
                wait = 2**attempt
                print(f"  ⚠ {desc} 重试 {attempt + 1}/{MAX_RETRIES}（{wait}s 后）: {exc}",
                      file=sys.stderr)
                time.sleep(wait)
            else:
                raise
    # 理论上不会走到这里（最后一次已在循环中 raise），但 mypy 需要
    assert last_exc is not None  # pragma: no cover — unreachable mypy-required code (loop always returns or raises)
    raise last_exc  # pragma: no cover — unreachable mypy-required code (loop always returns or raises)


# ── 交易所代码推断 ────────────────────────────────────────────────────────


def infer_exchange(code: str) -> str:
    """从 6 位 A 股代码前缀推断交易所。

    规则（需按顺序检查）：
        "6"*  → SH（上交所，600/601/603/605/688/689）
        "4"/"8"/"9"* → BJ（北交所）
        其他 → SZ（深交所）
    """
    if code.startswith("6"):
        return "SH"
    if code.startswith(("4", "8", "9")):
        return "BJ"
    return "SZ"


# ── SSE JSON 解析 ─────────────────────────────────────────────────────────


def parse_sse_json(data: dict[str, Any], update_date: str) -> list[dict[str, Any]]:
    """解析上交所 JSON 响应，仅保留 A 股（STOCK_TYPE=1/8），过滤 B 股。

    Args:
        data: 上交所 API 返回的完整 JSON（含 pageHelp.data[]）
        update_date: 更新日期（YYYY-MM-DD）

    Returns:
        统一 schema 的记录列表。退市股保留（DELIST_DATE != "-" 时设置退市日期）。
        空载荷返回 []。
    """
    records: list[dict[str, Any]] = []
    rows = data.get("pageHelp", {}).get("data", [])
    if not isinstance(rows, list):
        return records

    for row in rows:
        stock_type = str(row.get("STOCK_TYPE", ""))
        if stock_type not in ("1", "8"):
            continue  # 过滤 B 股（STOCK_TYPE="2"）及其他

        code = str(row.get("A_STOCK_CODE", ""))
        list_raw = str(row.get("LIST_DATE", ""))
        delist_raw = str(row.get("DELIST_DATE", "-"))

        # 日期转换：YYYYMMDD → YYYY-MM-DD
        list_date = _fmt_date(list_raw)
        delist_date = "" if delist_raw == "-" else _fmt_date(delist_raw)

        # 板块：1=主板，2=科创板
        board = "科创板" if str(row.get("LIST_BOARD", "")) == "2" else "主板"

        # 公司全称：优先 FULL_NAME，缺失回退 SEC_NAME_FULL
        full_name = row.get("FULL_NAME") or row.get("SEC_NAME_FULL", "")

        records.append({
            "symbol": f"SH{code}",
            "ts_code": f"{code}.SH",
            "code": code,
            "name": str(row.get("COMPANY_ABBR", "")),
            "exchange": "SH",
            "list_date": list_date,
            "delist_date": delist_date,
            "board": board,
            "full_name": str(full_name),
            "total_share": "",
            "industry": str(row.get("CSRC_CODE_DESC", "")),
            "region": str(row.get("AREA_NAME_DESC", "")),
            "update_date": update_date,
        })

    return records


# ── SZSE XLSX 解析（正常上市） ────────────────────────────────────────────


def parse_szse_xlsx(sheet_xml: str, update_date: str) -> list[dict[str, Any]]:
    """解析深交所正常上市股票 xlsx（CATALOGID=1110）。

    Args:
        sheet_xml: xl/worksheets/sheet1.xml 的原始文本（22 列）
        update_date: 更新日期

    Returns:
        统一 schema 的记录列表。跳过表头行和空行。空 sheet 返回 []。

    列映射（0-indexed）：
        0=A板块  1=B公司全称  4=E代码  5=F简称  6=G上市日期
        7=H总股本(数字)  15=P省份  17=R行业
    """
    records: list[dict[str, Any]] = []
    rows = _ROW_RE.findall(sheet_xml)

    for i, row_xml in enumerate(rows):
        if i == 0:  # 跳过表头
            continue

        cells = _parse_xlsx_cells(row_xml)
        code = cells.get("E", "").strip()
        if not code:
            continue  # 跳过空行

        # 总股本：数字单元格，去千分位逗号转 float；缺失则留空
        ts_raw = cells.get("H", "").strip().replace(",", "")
        total_share: str | float = float(ts_raw) if ts_raw else ""

        records.append({
            "symbol": f"SZ{code}",
            "ts_code": f"{code}.SZ",
            "code": code,
            "name": cells.get("F", ""),
            "exchange": "SZ",
            "list_date": cells.get("G", ""),  # 已为 YYYY-MM-DD
            "delist_date": "",
            "board": cells.get("A", ""),
            "full_name": cells.get("B", ""),
            "total_share": total_share,
            "industry": cells.get("R", ""),
            "region": cells.get("P", ""),
            "update_date": update_date,
        })

    return records


# ── SZSE XLSX 解析（退市） ────────────────────────────────────────────────


def parse_szse_delisted(sheet_xml: str, update_date: str) -> list[dict[str, Any]]:
    """解析深交所退市股票 xlsx（CATALOGID=1793_ssgs，TABKEY=tab2）。

    Args:
        sheet_xml: sheet1.xml 文本（4 列：代码/简称/上市日/终止上市日）
        update_date: 更新日期

    Returns:
        统一 schema 记录；board/full_name/total_share/industry/region 均为空。
    """
    records: list[dict[str, Any]] = []
    rows = _ROW_RE.findall(sheet_xml)

    for i, row_xml in enumerate(rows):
        if i == 0:
            continue
        cells = _parse_xlsx_cells(row_xml)
        code = cells.get("A", "").strip()
        if not code:
            continue

        records.append({
            "symbol": f"SZ{code}",
            "ts_code": f"{code}.SZ",
            "code": code,
            "name": cells.get("B", ""),
            "exchange": "SZ",
            "list_date": cells.get("C", ""),
            "delist_date": cells.get("D", ""),
            "board": "",
            "full_name": "",
            "total_share": "",
            "industry": "",
            "region": "",
            "update_date": update_date,
        })

    return records


# ── BSE JSON 解析 ─────────────────────────────────────────────────────────


def parse_bse_json(body: str, update_date: str) -> list[dict[str, Any]]:
    """解析北交所 JSONP 响应 body（null([{...}]) 包裹格式）。

    Args:
        body: 完整 API 响应文本，以 "null(" 开始、")" 结束
        update_date: 更新日期

    Returns:
        统一 schema 记录列表。空 body "null([])" 返回 []。
    """
    records: list[dict[str, Any]] = []

    # 剥离 JSONP 包裹：null( ... )
    if not body.startswith("null(") or not body.endswith(")"):
        return records
    inner = body[5:-1]  # 去掉 "null(" 和 ")"

    data: list[dict[str, Any]] = json.loads(inner)
    if not data:
        return records

    # data[0] 包含分页信息与 content 数组
    content = data[0].get("content", [])
    if not isinstance(content, list):
        return records

    for row in content:
        code = str(row.get("xxzqdm", ""))
        list_raw = str(row.get("fxssrq", ""))
        xzgb: Any = row.get("xxzgb")

        total_share: str | float = float(xzgb) if xzgb is not None else ""

        records.append({
            "symbol": f"BJ{code}",
            "ts_code": f"{code}.BJ",
            "code": code,
            "name": str(row.get("xxzqjc", "")),
            "exchange": "BJ",
            "list_date": _fmt_date(list_raw),
            "delist_date": "",
            "board": "北交所",
            "full_name": "",
            "total_share": total_share,
            "industry": str(row.get("xxhyzl", "")),
            "region": str(row.get("xxssdq", "")),
            "update_date": update_date,
        })

    return records


# ── 合并与去重 ────────────────────────────────────────────────────────────


def merge_exchanges(record_lists: list[list[dict[str, Any]]]) -> list[dict[str, Any]]:
    """合并多个交易所记录，按 code 去重（保留首次出现），按 code 升序排序。

    Args:
        record_lists: 各交易所的记录列表

    Returns:
        去重合并后的记录列表。空输入返回 []。
    """
    seen: dict[str, dict[str, Any]] = {}
    for records in record_lists:
        for r in records:
            code = r.get("code", "")
            if code and code not in seen:
                seen[code] = r
    return sorted(seen.values(), key=lambda r: r["code"])


# ── CSV 输出 ──────────────────────────────────────────────────────────────


def records_to_csv(records: list[dict[str, Any]], path: Path) -> None:
    """将记录列表写入 CSV 文件，UTF-8 BOM 编码，列顺序 = COLUMNS。

    Args:
        records: 要写入的记录列表
        path: 输出文件路径

    空记录列表 → 仅输出表头。
    """
    with open(path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.DictWriter(
            f, fieldnames=COLUMNS, extrasaction="ignore", lineterminator="\n"
        )
        writer.writeheader()
        for r in records:
            writer.writerow(r)


# ── 网络抓取函数（非测试范围，供 main() 在线调用） ─────────────────────────


def fetch_sse(session: requests.Session) -> dict[str, Any]:
    """从上交所 API 获取 A 股股票列表 JSON。

    Returns:
        上交所 JSON 响应（含 pageHelp.data[]）
    """
    headers = {
        "User-Agent": USER_AGENT,
        "Referer": "https://www.sse.com.cn/",
    }
    resp = session.get(SSE_URL, params=SSE_PARAMS, headers=headers, timeout=30)
    resp.raise_for_status()
    return resp.json()


def fetch_szse_xlsx(session: requests.Session, catalogid: str, tabkey: str) -> str:
    """从深交所 API 下载 xlsx 报表，解压提取 sheet1.xml。

    Args:
        session: HTTP 会话
        catalogid: CATALOGID 参数（1110=正常上市，1793_ssgs=退市）
        tabkey: TABKEY 参数（tab1/tab2）

    Returns:
        xl/worksheets/sheet1.xml 的文本内容
    """
    params: dict[str, str] = {
        "SHOWTYPE": "xlsx",
        "CATALOGID": catalogid,
        "TABKEY": tabkey,
        "random": "0.1",
    }
    headers = {
        "User-Agent": USER_AGENT,
        "Referer": "https://www.szse.cn/",
    }
    resp = session.get(SZSE_XLSX_URL, params=params, headers=headers, timeout=30)
    resp.raise_for_status()

    # xlsx 本质是 ZIP 包，提取 sheet1.xml
    with zipfile.ZipFile(BytesIO(resp.content)) as zf:
        return zf.read("xl/worksheets/sheet1.xml").decode("utf-8")


def fetch_bse(session: requests.Session) -> list[dict[str, Any]]:
    """从北交所 API 分页获取全部上市股票。

    流程：先 GET 列表页获取 cookie，再 POST 分页请求，
    解析 null([...]) JSONP 响应，累积 content[] 行。

    Returns:
        原始 API 行列表（字段：xxzqdm, xxzqjc, fxssrq, xxhyzl, xxssdq, xxzgb 等）
    """
    # 第一步：访问列表页获取必要 cookie
    session.get(
        BSE_LISTED_URL,
        headers={"User-Agent": USER_AGENT},
        timeout=20,
    )

    all_rows: list[dict[str, Any]] = []
    page = 0

    while True:
        data: dict[str, str | list[str]] = {
            "page": str(page),
            "typejb": "T",
            "xxfcbj[]": "2",
            "xxzqdm": "",
            "sortfield": "xxzqdm",
            "sorttype": "asc",
        }
        headers = {
            "User-Agent": USER_AGENT,
            "Content-Type": "application/x-www-form-urlencoded",
            "Referer": "https://www.bse.cn/nq/listedcompany.html",
            "X-Requested-With": "XMLHttpRequest",
        }
        resp = session.post(BSE_API_URL, data=data, headers=headers, timeout=30)
        resp.raise_for_status()
        body = resp.text

        # 解析 JSONP: null([{...}])
        inner = body[5:-1]  # 去掉 null( 和 )
        wrapper: list[dict[str, Any]] = json.loads(inner)
        if not wrapper:
            break

        meta = wrapper[0]
        content = meta.get("content", [])
        if not isinstance(content, list) or not content:
            break

        all_rows.extend(content)

        total_pages: int = meta.get("totalPages", 0)
        page += 1
        if page >= total_pages:
            break

    return all_rows


# ── 主入口 ────────────────────────────────────────────────────────────────


def main() -> None:
    """命令行入口：从三大交易所抓取股票基本信息并合并输出 CSV。"""
    parser = argparse.ArgumentParser(
        description="从三大交易所官方网站抓取 A 股股票基本信息",
    )
    parser.add_argument(
        "-o", "--output",
        default="stock_basic_official.csv",
        help="输出 CSV 路径（默认: stock_basic_official.csv）",
    )
    parser.add_argument(
        "--update-date",
        default=date.today().isoformat(),
        help="更新日期 YYYY-MM-DD（默认: 今天）",
    )
    args = parser.parse_args()

    update_date: str = args.update_date
    output_path = Path(args.output)

    # 验证日期格式
    try:
        datetime.strptime(update_date, "%Y-%m-%d")
    except ValueError:
        print(f"错误：日期格式无效 '{update_date}'，需要 YYYY-MM-DD", file=sys.stderr)
        sys.exit(1)

    print("开始从三大交易所抓取股票基本信息...", file=sys.stderr)
    print(f"  更新日期: {update_date}", file=sys.stderr)

    session = requests.Session()
    session.headers.update({"User-Agent": USER_AGENT})

    # 收集各交易所记录
    exchange_records: list[list[dict[str, Any]]] = []

    # ── 上交所 ──
    print("\n[1/4] 上交所 SSE ...", file=sys.stderr)
    try:
        sse_data = _with_retry(fetch_sse, session, desc="上交所")
        sse_records = parse_sse_json(sse_data, update_date)
        exchange_records.append(sse_records)
        print(f"  ✓ 上交所: {len(sse_records)} 条（含退市）", file=sys.stderr)
    except Exception as exc:
        print(f"  ✗ 上交所失败: {exc}", file=sys.stderr)

    # ── 深交所（正常上市） ──
    print("\n[2/4] 深交所 SZSE 正常上市 ...", file=sys.stderr)
    try:
        szse_active_xml = _with_retry(
            fetch_szse_xlsx, session, "1110", "tab1", desc="深交所 正常上市"
        )
        szse_records = parse_szse_xlsx(szse_active_xml, update_date)
        exchange_records.append(szse_records)
        print(f"  ✓ 深交所 正常上市: {len(szse_records)} 条", file=sys.stderr)
    except Exception as exc:
        print(f"  ✗ 深交所 正常上市 失败: {exc}", file=sys.stderr)

    # ── 深交所（退市） ──
    print("\n[3/4] 深交所 SZSE 退市股 ...", file=sys.stderr)
    try:
        szse_delisted_xml = _with_retry(
            fetch_szse_xlsx, session, "1793_ssgs", "tab2", desc="深交所 退市"
        )
        szse_delisted_records = parse_szse_delisted(szse_delisted_xml, update_date)
        exchange_records.append(szse_delisted_records)
        print(f"  ✓ 深交所 退市: {len(szse_delisted_records)} 条", file=sys.stderr)
    except Exception as exc:
        print(f"  ✗ 深交所 退市 失败: {exc}", file=sys.stderr)

    # ── 北交所 ──
    print("\n[4/4] 北交所 BSE ...", file=sys.stderr)
    try:
        bse_raw_rows = _with_retry(fetch_bse, session, desc="北交所")
        # 将原始行重新打包为 JSONP body 再调用 parse_bse_json
        bse_body = f"null([{{\"content\": {json.dumps(bse_raw_rows)}}}])"
        bse_records = parse_bse_json(bse_body, update_date)
        exchange_records.append(bse_records)
        print(f"  ✓ 北交所: {len(bse_records)} 条", file=sys.stderr)
    except Exception as exc:
        print(f"  ✗ 北交所失败: {exc}", file=sys.stderr)

    # ── 合并、去重、排序 ──
    print("\n合并各交易所数据...", file=sys.stderr)
    merged = merge_exchanges(exchange_records)
    print(f"  合并后: {len(merged)} 条（去重后）", file=sys.stderr)

    # ── 输出 CSV ──
    records_to_csv(merged, output_path)
    print(f"\n✓ 完成 — {len(merged)} 条 → {output_path.resolve()}", file=sys.stderr)


if __name__ == "__main__":  # pragma: no cover — __main__ block, never executed under pytest
    main()
