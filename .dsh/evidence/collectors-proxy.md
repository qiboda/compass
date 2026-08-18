# Evidence — collectors-proxy (#294)

> 真实冒烟与门禁证据。时间：2026-08-19 00:00–00:03 (Asia/Shanghai)。
> 环境：DSH worktree sandbox（无 proxy_pool/Redis/Docker 容器；外网可达）。

## 1. 测试门禁（pytest + coverage）

- 命令：`cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95 --cov-report=term-missing -q`
- 结果：**830 passed**，`TOTAL 2882 stmts, 98 miss, 96.60%`，`Required test coverage of 95% reached`。
- ruff：`uv run ruff check *.py tests/` 通过（新增/修改文件全绿）。

## 2. keepalive 真实单轮（`--once`）

命令：
```bash
cd collectors
timeout 90 uv run python proxy_keepalive.py --once \
  --snapshot /tmp/freeproxy-smoke.json --limit 5 --realtime-sources ""
```

输出：
```
[keepalive] json source ok (308212 bytes)
[keepalive] json cycle error: Error 111 connecting to 127.0.0.1:6379. Connection refused.
[keepalive] realtime source produced no records
[keepalive] cycle done: json=0 realtime=0
PIPESTATUS=0
```

结论：
- freeproxy `proxies.json` 真实下载成功（~308KB），快照写入 `/tmp/freeproxy-smoke.json`；
- proxy_pool Redis 不可达（沙箱无 Redis）→ `run_cycle` 捕获 `json cycle error`，进程**不崩溃**、退出码 0；
- realtime 源为空时优雅记 0。

## 3. collector 代理降级真实路径

命令（真实网络，池不可达）：
```bash
COMPASS_CSV_DIR=/tmp/smoke-csv timeout 90 uv run python - <<'PY'
import asyncio
import fetch_main_flow
async def main():
    await fetch_main_flow.run(page_size=100)
asyncio.run(main())
PY
```

输出（截取）：
```
Report: RPT_MAIN_MONEY_FLOW (EastMoney push2 clist f62)
Output: /tmp/smoke-csv/RPT_MAIN_MONEY_FLOW.csv
[proxy] WARN/ERROR: https pool empty, falling back to direct
```

`/tmp/smoke-csv/proxy_pool_state.json`：
```json
{
  "timestamp": "2026-08-19T00:02:23",
  "pool_count": 0,
  "degraded": true,
  "reason": "proxy_pool API unreachable: Failed to perform, curl: (7) Failed to connect to 127.0.0.1 port 5010 after 0 ms: Could not connect to server. ..."
}
```

结论：
- 真实 collector（`fetch_main_flow`）在 proxy_pool 不可达时按契约：醒目打印降级警告 + 写
  `proxy_pool_state.json`（时间戳/池计数=0/降级=true/原因）+ 直连；
- push2 直连在沙箱超时（exit 124），属沙箱网络限制；真实 VPS 上 push2his/THS 的
  curl 56 断连正是本 issue 要解决的场景，需在**装有 proxy_pool 的生产机**上做最终
  端到端验证（喂池后一次东财/THS 拉取）。

## 4. 已知限制 / 待生产验证

- 本沙箱无 proxy_pool 容器与 Redis，**无法验证真实代理成功路径**（有代理→走代理→
  坏代理 delete→轮换）。该路径由 77 个单元测试（stub ProxyPool/StubSession）覆盖。
- 生产验证清单：
  1. 启动 `scripts/proxy_pool/docker-compose.yml`（或现有容器）并确认 `/count/` 有 https 代理；
  2. 后台跑 `proxy_keepalive.py --interval 600` 保持池温；
  3. 跑一次 `fetch_index_daily`（或 `fetch_main_flow`）确认走代理且无 curl 56；
  4. 观察 `proxy_pool_state.json` 在池空时降级、池有货时不降级。
