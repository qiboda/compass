# Plan — proxy-pool-trial (#287)

## 用途

试用 `jhao104/proxy_pool` 作为 collectors 的代理方案：先用独立验证脚本确认免费代理能否通过 THS 板块接口（10jqka），再决定是否接入 `fetch_index_daily.py` 的 THS 板块部分。

**Issue**: https://github.com/qiboda/compass/issues/287
**分支**: `feat/proxy-pool-trial`
**原始分支**: `master`

## 已锁定决策（grill-me 共识）

1. 目标：给现有 collectors 增加代理能力，先验证 proxy_pool 是否可用。
2. 部署：proxy_pool + Redis 用 Docker Compose 跑在本机（本机有 Docker，无 Redis）。
3. 先做独立验证脚本，不改现有 collectors。
4. 验证客户端：`curl_cffi`（与 collectors 相同的 TLS 指纹）。
5. 验证目标：THS 行业列表页 `https://q.10jqka.com.cn/thshy/` + 一个板块 kline（如 `https://d.10jqka.com.cn/v4/line/bk_881101/01/<今年>.js`），各 15 次共 30 次。
6. 通过标准：成功率 ≥ 50% 且平均耗时 < 5s。
7. 验证通过后：先接入 `fetch_index_daily.py` 的 THS 板块部分试点（本次不实现）。
8. 验证不通过：先改用 Vultr 单 IP 做同样验证；再不行上付费 API（本次不实现）。

## 范围（本次 PR）

- `scripts/proxy_pool/docker-compose.yml`：本地启动 proxy_pool + Redis。
- `collectors/check_proxy_pool.py`：独立验证脚本，从 proxy_pool API 取代理，用 `curl_cffi` 打 THS 列表页 + 一个板块 kline，输出成功率/平均耗时/失败原因并判定。
- 对应测试（RED 先行）：需求测试 + 对抗性测试。
- 文档同步（5b）。
- 运行真实 30 次验证并汇报结果。

不在范围：正式接入 `fetch_index_daily.py`。

## Tasks

### Batch 1 — 验证工具链
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #287 | 委派 adversarial test 子代理写 RED（脚本接口契约） | — |
| done | #287 | 委派 requirement test 子代理写 RED（需求验收） | — |
| done | #287 | 实现 `scripts/proxy_pool/docker-compose.yml`（redis + proxy_pool） | RED 测试 |
| done | #287 | 实现 `collectors/check_proxy_pool.py`（代理获取/请求/统计/判定） | RED 测试 |
| done | #287 | 全量 Python 测试 + 覆盖率门禁 | 实现 |
| done | #287 | 本地 Docker 启动 proxy_pool，真实 30 次验证并汇报 | 实现 |
| done | #287 | 文档同步（`.dsh/kb/dev/process.md` 或 toolchain） | 实现 |

## 验收标准

1. `scripts/proxy_pool/docker-compose.yml` 可一键启动 proxy_pool + Redis（本机 Docker）。
2. `collectors/check_proxy_pool.py` 可运行，从 proxy_pool API 取代理，完成 30 次请求，输出成功率/平均耗时/失败原因，并按锁定标准给出 PASS/FAIL。
3. 不修改现有 collectors 采集逻辑。
4. 测试覆盖：代理获取、请求统计、判定逻辑、参数/异常路径。
5. 全套件绿：`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q`。
