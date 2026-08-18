# Plan — collectors-proxy (#294)

## 用途

给全部 Python collectors 接入 freeproxy + proxy_pool 代理层（proxy-first + 池空降级 + 后台持续喂源），解决东财 push2his / THS 在 VPS IP 上的限流断连（curl 56）。

**Issue**: https://github.com/qiboda/compass/issues/294
**分支**: `feat/collectors-proxy`
**原始分支**: `master`

## 已锁定决策（grill-me 共识，契约）

1. **范围**：全部 collectors 的 HTTPS 源——东财 datacenter（fin_*/flow/survey/block_trade/dragon）+ push2his 官方指数 + THS 行业板块 + 交易所官网 stock_basic。
2. **策略**：proxy-first——有 https 代理必走代理；池空时醒目打印 `[proxy] WARN/ERROR: https pool empty, falling back to direct` + 写 `proxy_pool_state.json`（时间戳/池计数/是否降级）+ 降级直连（绝不忽略、绝不因无代理失败）；坏代理用后 `delete` 出池并换下一个（有界重试次数）；index_daily 保留现有腾讯 day 回退作为最后兜底。
3. **喂源（保持池温）**：新增后台常驻循环（keepalive），每周期跑 freeproxy `--source json` + `--source realtime` 双源灌 proxy_pool Redis（`use_proxy` hash）；GitHub raw 429/超时 → 跳过本轮，用本地 `/tmp/freeproxy.json` 快照兜底，不崩溃。
4. **通知**：仅日志报错（无 IM/webhook）。
5. 完整 PRE-IMPLEMENTATION GATE：plan → RED（adversarial + requirement 两批）→ 实现 → docs → 反思 → push → 关 issue。

## 技术方案

### 1. 新模块 `collectors/proxy_pool_client.py`

代理池客户端（独立、可测试、不反向依赖 common 以避免循环 import）：

- 常量：`DEFAULT_API_URL = "http://127.0.0.1:5010"`、`PROXY_STATE_FILENAME = "proxy_pool_state.json"`、`DEFAULT_PROXY_MAX_ATTEMPTS = 3`。
- 环境变量：
  - `COMPASS_PROXY_API_URL`：覆盖 proxy_pool API 基址。
  - `COMPASS_PROXY_DISABLE=1`：完全禁用代理层（测试/本地开发）。
- `class ProxyPool`：
  - `__init__(api_url=None, state_path=None)`：api_url 默认取环境变量或 DEFAULT；state_path 默认 `COMPASS_CSV_DIR`（或 `/data/compass-data/csv`）/`proxy_pool_state.json`。
  - `async get_proxy() -> str | None`：GET `/get/?type=https`；空池/API 异常/畸形响应 → `None` 并触发一次降级记录（首次醒目打印 + 写 state）。
  - `async delete_proxy(proxy: str) -> None`：GET `/delete/?proxy=IP:PORT`；失败仅日志，不抛出。
  - `async pool_count() -> int`：GET `/count/`；防御解析（dict/int/str），失败返回 0。
  - `record_state(pool_count, degraded, reason)`：原子 JSON 写 state（时间戳/池计数/是否降级/原因）。
  - `static proxy_spec(proxy) -> dict[str,str]`：`{"http": "http://ip:port", "https": "http://ip:port"}`。
  - 内部用 `curl_cffi.requests.get`（同步）经 `asyncio.to_thread` 调用，避免阻塞事件循环；测试 monkeypatch `_api_get`。
- `def proxy_enabled() -> bool`：读取 `COMPASS_PROXY_DISABLE`。
- `def default_state_path() -> Path`。

### 2. `common.py` 新增请求包装与集成点

- `make_proxy_pool() -> ProxyPool | None`：懒加载 `proxy_pool_client`，禁用时返回 None；默认 `state_path=csv_dir()/proxy_pool_state.json`。
- `async proxy_get(session, pool, url, **kwargs)`：proxy-first GET 包装：
  - `pool is None` → 直连（完全兼容旧行为）。
  - 尝试 `PROXY_MAX_ATTEMPTS` 个代理；每个代理请求异常 → `delete_proxy` 出池并换下一个。
  - 代理耗尽或池空 → 最终直连一次；直连异常原样抛出（交给各模块既有 retry 循环）。
  - HTTP 状态码（429/5xx 等）不是异常，返回 response，由调用方按现有逻辑处理。
- `async proxy_post(...)`：POST 同构包装（北交所 BSE 用）。
- `proxy_get_sync(session, pool, url, **kwargs)` / `proxy_post_sync(...)`：同步 `requests.Session` 版本（交易所官网用）。
- `fetch_paginated(..., *, pool=None)`：新增 keyword-only `pool`，内部改用 `proxy_get`；默认 None 保持所有现有测试/调用兼容。

### 3. 各 collector 接入（默认 proxy-first）

| 模块 | 接入点 |
|---|---|
| `fetch_balance_sheet.py` / `fetch_cash_flow.py` / `fetch_income.py` / `fetch_block_trade.py` / `fetch_dragon.py` / `fetch_institution_survey.py` | `run()` 创建 `pool=make_proxy_pool()`，传给 `fetch_paginated(..., pool=pool)`（6 个模块共用 common 单一入口） |
| `fetch_fin_indicators.py` | `fetch_by_report_date` / `fetch_by_update_date` 增加 `pool` 参数并改用 `proxy_get`；`run()` 创建并传递 |
| `fetch_main_flow.py` | `_fetch_page`（及其调用链）增加 `pool` 参数并改用 `proxy_get` |
| `fetch_index_daily.py` | `_get_json` / `fetch_ths_industry_list` / `fetch_ths_kline` 增加 `pool` 参数并改用 `proxy_get`；`fetch_kline` / `_fetch_tencent_kline` 经 `_get_json` 自动继承；`run()` 创建并传递；腾讯 day 回退保持为最后兜底 |
| `fetch_stock_basic_official.py` | `fetch_sse` / `fetch_szse_xlsx` / `fetch_bse` 增加 `pool` 参数并改用 `proxy_get_sync` / `proxy_post_sync`；`main()` 创建并传给 `_with_retry` |

### 4. `fetch_freeproxy.py` 小重构（快照兜底支持）

- 拆出 `fetch_json_payload(url) -> Any`（GET + json）与 `records_from_json_data(payload, limit) -> list[dict]`（现有解析/过滤/归一化逻辑）。
- `fetch_json_proxies(url, limit)` 改为组合二者，行为不变，现有测试保持绿。

### 5. 新脚本 `collectors/proxy_keepalive.py`

后台常驻 keepalive（保持池温）：

- CLI：`--interval 600`（秒，默认 600）、`--once`、`--json-url`、`--snapshot /tmp/freeproxy.json`、`--redis-url`、`--table use_proxy`、`--limit 300`、`--realtime-sources`。
- 每周期：
  1. JSON 源：下载 `proxies.json` → 成功则存快照到 `--snapshot` 并灌 Redis；
  2. 下载失败（GitHub raw 429/超时等）→ 读本地快照兜底灌 Redis；无快照则本轮 JSON 源记 0；
  3. realtime 源：`fetch_freeproxy.fetch_realtime_proxies` + `write_to_redis`；
  4. 打印本轮汇总；`--once` 退出，否则 sleep interval。
- 每个子步骤独立 try/except，任何异常只日志不崩溃。

## Tasks

### Batch 1 — RED 测试（实现前）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | 对抗性 RED 测试（空池/API 不可达/畸形响应/坏代理 delete 与轮换/有界重试耗尽/直连兜底/禁用开关/state 原子性/keepalive 429 快照/循环不崩溃）——**子代理基础设施不可用，经用户批准由主 agent fallback 自写**（记录于 toolchain.md） | — |
| done | #294 | 需求验收 RED 测试（proxy-first 契约、空池降级、index_daily 腾讯兜底保留、stock_basic 同步代理、keepalive 双源 + 快照）——同上 fallback | — |

### Batch 2 — 代理客户端与请求包装
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | 实现 `collectors/proxy_pool_client.py` | Batch 1 |
| done | #294 | `common.py` 增加 `make_proxy_pool` / `proxy_get` / `proxy_post` / 同步版本 / `fetch_paginated(pool=...)` | 上 |
| done | #294 | 全量 pytest + 覆盖率门禁（RED 两批转绿） | 上 |

### Batch 3 — 各 collector 接入
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | datacenter 6 模块接入（balance_sheet/cash_flow/income/block_trade/dragon/institution_survey） | Batch 2 |
| done | #294 | `fetch_fin_indicators` + `fetch_main_flow` 接入 | Batch 2 |
| done | #294 | `fetch_index_daily` 接入（push2his/THS/腾讯兜底） | Batch 2 |
| done | #294 | `fetch_stock_basic_official` 同步代理接入 | Batch 2 |
| done | #294 | 全量 pytest + 覆盖率门禁 | 上 |

### Batch 4 — keepalive 喂源
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | `fetch_freeproxy.py` 拆 `fetch_json_payload` / `records_from_json_data`（保持现有测试绿） | Batch 2 |
| done | #294 | 实现 `collectors/proxy_keepalive.py`（双源 + 快照兜底 + --once） | 上 |
| done | #294 | 新增 keepalive/重构测试 + 全量 pytest + 覆盖率门禁 | 上 |

### Batch 5 — 文档同步 + 决策记录
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | `.dsh/kb/design/data-providers.md`：新增代理/网络章节 + `## 决策记录` | Batch 3/4 |
| done | #294 | `.dsh/kb/user/cli.md`：keepalive 命令 + 环境变量 | Batch 3/4 |
| done | #294 | `.dsh/kb/dev/process.md`：keepalive 运维 Runbook | Batch 3/4 |
| done | #294 | 全仓 grep 新标识符（proxy_keepalive、COMPASS_PROXY_*、proxy_pool_state）核对引用 | 上 |

### Batch 6 — 门禁全跑 + 冒烟 + 收尾
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #294 | `uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` + ruff | Batch 5 |
| done | #294 | 真实冒烟：keepalive `--once`（json 下载 + Redis 缺失优雅降级）+ 真实 collector 代理降级路径，evidence 已落 `.dsh/evidence/collectors-proxy.md` | 上 |
| in_progress | #294 | commit → review（可多轮） | 上 |
| pending | #294 | push 前 rebase + `skwy-reflect` 反思 commit | review |
| pending | #294 | 用户确认 push → push → PR → 合并后 issue 收尾（完成 comment + close） | 上 |

## 验收标准

1. `collectors/proxy_pool_client.py` 提供 get/delete/count、空池降级状态文件、坏代理轮换（有界重试）、禁用开关。
2. 所有范围内 collectors 默认 proxy-first：有 https 代理必走代理；池空/API 不可达 → 醒目警告 + `proxy_pool_state.json` + 直连不失败。
3. 坏代理请求异常 → `/delete/` 出池 + 换下一个；超过有界次数后降级直连。
4. `index_daily` 保留腾讯 day 回退作为最后兜底。
5. `collectors/proxy_keepalive.py` 支持双源喂池 + `/tmp/freeproxy.json` 快照兜底 + `--once`。
6. RED 两批测试在实现前写入；实现后全绿；`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` 通过。
7. 真实冒烟 evidence 落盘。
8. 文档同步完成（data-providers / cli / process + 决策记录）。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 代理注入粒度 | 会话级绑定 / 每请求级 `proxies` 参数 | 每请求级包装（`proxy_get` 等） | curl_cffi 支持每请求 `proxies`；坏代理可精确 delete 并换下一个，不重建会话 | 会话级绑定无法在同一请求内轮换坏代理，重建 AsyncSession 开销大且复杂 |
| 降级状态文件位置 | `/tmp` / `csv_dir()` | `csv_dir()/proxy_pool_state.json`（`COMPASS_CSV_DIR` 可覆盖） | 与 progress 文件一致、可测试、生产持久可见 | `/tmp` 易失且测试隔离难 |
| 池空时行为 | 抛错 / 静默直连 / 醒目警告+直连 | 醒目警告+写 state+直连 | 锁定决策要求绝不因无代理失败，但降级必须可观测 | 抛错违背"绝不因无代理失败"；静默直连违背可观测性 |
| 坏代理判定 | HTTP 非 2xx / 仅请求异常 | 仅请求异常（网络/CONNECT/超时）触发 delete+轮换 | 429/5xx 多为服务端限流或业务错误，不应误杀可用代理 | HTTP 状态码由各模块既有 retry 处理，误删会降低池质量 |
| 有界重试后 | 放弃请求 / 直连兜底 | 直连兜底一次，失败交给模块既有 retry | 锁定决策"池空/坏代理均降级直连" | 放弃请求会导致数据缺失，违反不因代理失败 |
| keepalive 实现 | 独立进程脚本 / 并入 main.py | 独立 `proxy_keepalive.py` + `--once` | 可后台常驻也可单轮测试/冒烟，职责单一 | 并入 main.py 增加 CLI 复杂度，且 sync 不应被 keepalive 阻塞 |
| freeproxy 快照兜底 | keepalive 自写下载逻辑 / 重构 fetch_freeproxy 拆解 | 拆 `fetch_json_payload` + `records_from_json_data` 复用 | 单一解析/过滤/归一化实现，快照与在线源同路径 | keepalive 自写会重复逻辑，后续变更易漂移 |
